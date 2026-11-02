import { nextUp } from "./attention.ts";
import type { ProviderId } from "./plan.ts";
import type { CardState, CardView, RunView } from "./snapshot.ts";

/**
 * Which projects the sidebar lists and what each run row says about its cards (ADR-0009).
 * Pure, like snapshot.ts: the list comes from `o projects --json`, runs from `o runs --json`.
 */
export type ProjectKind = "root" | "test" | "registered";

/** One folder `o projects --json` lists. */
export interface ListedProject {
  path: string;
  name: string;
  kind: ProjectKind;
  /** A registered folder that is gone; it stays so it can be taken off. */
  missing?: boolean;
}

export interface ProjectList {
  root: string;
  defaultRoot: boolean;
  rootExists: boolean;
  projects: ListedProject[];
}

/** `o projects check --json`: whether a folder may be listed, and why not. */
export interface ProjectCheck {
  path: string;
  ok: boolean;
  reason?: string;
}

/** A folder as O compares it: `/` slashes, no trailing one, and a Windows path in any case. */
export function folderKey(path: string): string {
  const slashes = path.trim().replace(/\\/g, "/").replace(/(.)\/+$/, "$1");
  return /^([A-Za-z]:|\/\/)/.test(slashes) ? slashes.toLowerCase() : slashes;
}

export function sameFolder(a: string, b: string): boolean {
  return folderKey(a) === folderKey(b);
}

/** Whether `path` is `dir` or inside it. */
export function within(path: string, dir: string): boolean {
  const p = folderKey(path);
  const d = folderKey(dir);
  return p === d || p.startsWith(d.endsWith("/") ? d : `${d}/`);
}

/** The listed project a folder belongs to: that folder, else the deepest listed folder holding it. */
export function ownerOf<T extends { path: string }>(listed: readonly T[], path: string): T | undefined {
  let best: T | undefined;
  for (const project of listed) {
    if (within(path, project.path) && (!best || folderKey(project.path).length > folderKey(best.path).length)) best = project;
  }
  return best;
}

/** Runs (and Claude Code sessions) whose folder is a listed project or inside one. The rest stay in the history, unshown. */
export function listedRuns<R extends Pick<RunView, "project">>(listed: readonly { path: string }[], runs: readonly R[]): R[] {
  return runs.filter((run) => ownerOf(listed, run.project));
}

/** Temp and scratch folders: history from them is never offered as a project. */
const SCRATCH = /\/(?:appdata\/local\/temp|tmp|temp)(?:\/|$)/i;

/**
 * "기록에서 찾은 프로젝트": the distinct folders history worked in that the list does not
 * cover yet, newest first, without temp folders. The machine checks each again
 * (`projectsCheck`) before the card offers it: the home folder and O's own are refused there.
 */
export function importCandidates(paths: readonly string[], listed: readonly { path: string }[], root?: string): string[] {
  const seen = new Set<string>();
  const found: string[] = [];
  for (const path of paths) {
    const key = folderKey(path);
    if (!key || seen.has(key)) continue;
    seen.add(key);
    if (!/^([a-z]:\/|\/)/i.test(key) || SCRATCH.test(key)) continue;
    if (ownerOf(listed, path) || (root && within(path, root))) continue;
    found.push(path);
  }
  return found;
}

/** How a run row sums up its cards: the colors are the state dot's. */
export type SummaryState = "done" | "running" | "waiting" | "failed" | "stopped";

export interface ChipSummary {
  /** One entry per agent, in the order its first card appears: "Codex ×4". */
  providers: { provider: ProviderId; count: number }[];
  /** Counts per state, in a fixed reading order, zeros left out. */
  states: { state: SummaryState; count: number }[];
  /** Cards a person should look at, shown one by one so they are never folded away. */
  attention: { card: CardView; why: AttentionKind }[];
}

export type AttentionKind = "failed" | "answer" | "silent" | "limit";

const SUMMARY_OF: Record<CardState, SummaryState> = { done: "done", running: "running", queued: "waiting", waiting: "waiting", failed: "failed", stopped: "stopped" };
const SUMMARY_ORDER: SummaryState[] = ["done", "running", "waiting", "failed", "stopped"];
const ATTENTION_ORDER: AttentionKind[] = ["failed", "answer", "silent", "limit"];

export function chipSummary(run: RunView, now: Date): ChipSummary {
  const providers: ChipSummary["providers"] = [];
  const counts = new Map<SummaryState, number>();
  for (const card of run.cards) {
    const entry = providers.find((p) => p.provider === card.provider);
    if (entry) entry.count += 1;
    else providers.push({ provider: card.provider, count: 1 });
    const state = SUMMARY_OF[card.state];
    counts.set(state, (counts.get(state) ?? 0) + 1);
  }
  // A card asking the lead, failing, gone quiet or held by a usage limit: the same signals as 다음 할 일.
  const { todo, stopped } = nextUp([run], now);
  const why = new Map<string, AttentionKind>();
  for (const item of [...todo, ...stopped]) {
    const kind = item.kind as AttentionKind;
    if (!item.cardId || !ATTENTION_ORDER.includes(kind)) continue;
    const had = why.get(item.cardId);
    if (!had || ATTENTION_ORDER.indexOf(kind) < ATTENTION_ORDER.indexOf(had)) why.set(item.cardId, kind);
  }
  const attention = run.cards.filter((c) => why.has(c.id)).map((card) => ({ card, why: why.get(card.id) as AttentionKind }));
  return {
    providers,
    states: SUMMARY_ORDER.filter((s) => counts.has(s)).map((state) => ({ state, count: counts.get(state) ?? 0 })),
    attention,
  };
}

/** Lead thread ids start with their UTC start second (`20261102-091500-ab12`); the time it names, or undefined. */
export function threadStartedAt(threadId: string): string | undefined {
  const m = /^(\d{4})(\d{2})(\d{2})-(\d{2})(\d{2})(\d{2})-/.exec(threadId);
  return m ? `${m[1]}-${m[2]}-${m[3]}T${m[4]}:${m[5]}:${m[6]}.000Z` : undefined;
}

/** A run counts as recent while it runs or for this long after it ended. */
export const RECENT_DAYS = 7;

/** Whether a run is running or ended within RECENT_DAYS; older ones never open on their own. */
export function isRecent(run: Pick<RunView, "status" | "startedAt" | "endedAt">, now: Date): boolean {
  if (run.status === "running") return true;
  const at = Date.parse(run.endedAt ?? run.startedAt);
  return Number.isFinite(at) && now.getTime() - at <= RECENT_DAYS * 24 * 60 * 60_000;
}
