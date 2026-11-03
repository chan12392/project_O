import type { ProviderId } from "./plan.ts";
import type { CardStatus } from "./events.ts";
import { blockedReason, endWord, failedBeforeCards, type CardView, type RunView } from "./snapshot.ts";

/**
 * What needs a person next, read from runs alone (pure, like snapshot.ts).
 * Two lists: things to do next (an agent asked the lead, a PR to review, a
 * finished run to check, a card about to start) and work that stopped (failed,
 * stopped, hit a limit, or went quiet while still marked running).
 */
export type TodoKind = "answer" | "review" | "check" | "queued";
export type StoppedKind = "failed" | "silent" | "limit" | "stopped";

export interface NextItem {
  /** Stable across refreshes, so a person can tick it off. */
  key: string;
  kind: TodoKind | StoppedKind;
  runId: string;
  runTitle: string;
  cardId?: string;
  provider?: ProviderId;
  /** The card's label, or the run's title for run-wide items. */
  title: string;
  /** What to read before acting: the question, the reason it stopped, the PR. */
  detail?: string;
  /** When it started waiting; the ticket ages from here. */
  since: string;
  pr?: string;
  question?: boolean;
}

/** Claude Code sessions read as runs (`o runs --sessions`) carry ids that start with this. */
export const SESSION_PREFIX = "session-";

/** A Claude Code session (its main thread as the lead, its subagents as cards), not an O plan. */
export function isSessionRun(run: Pick<RunView, "runId">): boolean {
  return run.runId.startsWith(SESSION_PREFIX);
}

/** How a message to the lead asks for something: a question mark, "~할까요", "~주세요", "알려 ~". */
const ASKS = /[?？]|까요|할지|주세요|부탁|알려/;

/** A message to the lead that asks nothing: it only needs reading, not an answer. */
export function isNote(item: Pick<NextItem, "kind" | "detail" | "question">): boolean {
  return item.kind === "answer" && !(item.question ?? ASKS.test(item.detail ?? ""));
}

/**
 * Whether a ticket waits on a person: not a note that only needs reading, and not from a
 * Claude Code session (a read-only record O cannot act on). Badges count only these.
 */
export function needsAction(item: Pick<NextItem, "kind" | "detail" | "runId" | "question">): boolean {
  return !isNote(item) && !isSessionRun(item);
}

/**
 * The O command for what the app cannot do yet (answer a card, stop a run), for a person
 * to paste into a terminal; a secondary step beside opening the record. What a status
 * command would print, the record already shows, so failed or stopped work gets none. A
 * Claude Code session is no O run (no plan, events or process of O's), so its tickets get
 * none either: they are read-only records.
 */
export function ticketCommand(item: Pick<NextItem, "kind" | "runId" | "cardId">): { label: string; text: string } | undefined {
  if (isSessionRun(item)) return undefined;
  if (item.kind === "answer") return { label: "답장 명령 복사", text: `o msg send --run ${item.runId} --as lead --to ${item.cardId} "답장 내용"` };
  if (item.kind === "silent") return { label: "중지 명령 복사", text: `o stop ${item.runId}` };
  return undefined;
}

/** A card still marked running with nothing new for this long has gone quiet. */
export const SILENT_MS = 10 * 60_000;
/** Finished runs stay on the list to be checked for this long. */
const CHECK_WINDOW_MS = 24 * 60 * 60_000;

const PR_URL = /https:\/\/github\.com\/[\w.-]+\/[\w.-]+\/pull\/\d+/;

/** A pull request the card's agent reported, found in what it wrote. */
export function prOf(card: CardView): string | undefined {
  for (const text of [card.result ?? "", ...card.log.map((l) => l.text)]) {
    const match = PR_URL.exec(text);
    if (match) return match[0];
  }
  return undefined;
}

/** Every token a card's CLI reported: fresh input, cached input and output. */
export function tokensOf(card: CardView): number {
  return card.usage.input + card.usage.cached + card.usage.output;
}

/** The card's own last entry: messages it received say nothing about whether it is alive. */
function lastOwn(card: CardView): CardView["log"][number] | undefined {
  return card.log.findLast((l) => l.kind !== "team" || l.team?.from === card.id);
}

const TODO_ORDER: TodoKind[] = ["answer", "review", "check", "queued"];
const STOPPED_ORDER: StoppedKind[] = ["failed", "silent", "limit", "stopped"];

function byKindThenAge<K extends string>(order: K[]) {
  return (a: NextItem, b: NextItem) => order.indexOf(a.kind as K) - order.indexOf(b.kind as K) || a.since.localeCompare(b.since);
}

export function nextUp(runs: RunView[], now: Date, handled: ReadonlySet<string> = new Set()): { todo: NextItem[]; stopped: NextItem[] } {
  const todo: NextItem[] = [];
  const stopped: NextItem[] = [];
  const keep = (list: NextItem[], item: NextItem) => {
    if (!handled.has(item.key)) list.push(item);
  };

  for (const run of runs) {
    const base = { runId: run.runId, runTitle: run.title };
    const cardOf = new Map(run.cards.map((c) => [c.id, c]));
    if (failedBeforeCards(run)) {
      keep(stopped, { ...base, key: `failed:${run.runId}`, kind: "failed", title: run.title, detail: blockedReason(run), since: run.endedAt ?? run.startedAt });
    }

    // An agent wrote to the lead and the lead has not written back to it (or to everyone) since.
    const answeredAt = (party: string) => run.messages.findLast((m) => m.from === "lead" && (m.to === party || m.to === "all"))?.at ?? "";
    for (const m of run.messages) {
      if (m.to !== "lead" || m.at <= answeredAt(m.from)) continue;
      const card = cardOf.get(m.from);
      keep(todo, { ...base, key: `answer:${run.runId}:${m.from}:${m.at}`, kind: "answer", cardId: m.from, provider: card?.provider, title: card?.label ?? m.from, detail: m.text, since: m.at, question: m.kind === undefined ? undefined : m.kind === "question" });
    }

    for (const card of run.cards) {
      const item = { ...base, cardId: card.id, provider: card.provider, title: card.label };
      const pr = prOf(card);
      if (card.state === "done" && pr) keep(todo, { ...item, key: `review:${run.runId}:${card.id}`, kind: "review", detail: pr, pr, since: card.endedAt ?? run.startedAt });
      if ((card.state === "waiting" || card.state === "queued") && run.status === "running") {
        const queued = card.log.find((l) => l.kind === "queued");
        keep(todo, { ...item, key: `queued:${run.runId}:${card.id}`, kind: "queued", detail: queued?.text ?? "앞 카드가 끝나면 시작합니다", since: queued?.at ?? run.startedAt });
      }
      if (card.state === "failed") keep(stopped, { ...item, key: `failed:${run.runId}:${card.id}`, kind: "failed", detail: card.result ?? card.activity, since: card.endedAt ?? run.startedAt });
      if (card.state === "stopped") keep(stopped, { ...item, key: `stopped:${run.runId}:${card.id}`, kind: "stopped", detail: card.result ?? card.activity, since: card.endedAt ?? run.startedAt });
      if (card.state === "running") {
        const own = lastOwn(card);
        const sign = own?.at ?? card.startedAt;
        if (own?.kind === "limit") keep(stopped, { ...item, key: `limit:${run.runId}:${card.id}`, kind: "limit", detail: card.activity, since: own.at });
        else if (sign && now.getTime() - Date.parse(sign) > SILENT_MS) keep(stopped, { ...item, key: `silent:${run.runId}:${card.id}:${sign}`, kind: "silent", detail: card.activity, since: sign });
      }
    }

    // A run that finished cleanly still wants a look at what it made (a session's owner was there already).
    if (run.status === "done" && run.endedAt && !isSessionRun(run) && now.getTime() - Date.parse(run.endedAt) < CHECK_WINDOW_MS) {
      keep(todo, { ...base, key: `check:${run.runId}`, kind: "check", title: run.title, detail: `카드 ${run.cards.length}장 완료`, since: run.endedAt });
    }
  }
  return { todo: todo.sort(byKindThenAge(TODO_ORDER)), stopped: stopped.sort(byKindThenAge(STOPPED_ORDER)) };
}

/** What a project is called: the last folder of its path. */
export function projectName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

export type SuggestionKind = StoppedKind | "answer" | "review" | "idle";

/** One recommendation across projects, with the one-line reason it was picked. */
export interface Suggestion {
  key: string;
  kind: SuggestionKind;
  project: string;
  /** The card's label, or the project's name for an idle project. */
  title: string;
  reason: string;
  /** When it started waiting; oldest first within a tier. */
  since: string;
  /** The Next up ticket behind it; absent for an idle project. */
  item?: NextItem;
  /** Other cards' review tickets for the same PR, folded into this pick so one PR takes one slot. */
  samePr?: NextItem[];
}

/** A project with nothing new for this many days, and cards still unfinished, is worth picking back up. */
export const IDLE_DAYS = 3;
const DAY_MS = 24 * 60 * 60_000;

/** Stopped work first, then questions, then PRs, then idle projects. */
const TIER: Record<SuggestionKind, number> = { failed: 0, silent: 0, limit: 0, stopped: 0, answer: 1, review: 2, idle: 3 };

/** How long something has waited, as the reason says it: "방금", "5분째", "3시간째", "2일째". */
export function waited(since: string, now: Date): string {
  const minutes = Math.max(0, Math.floor((now.getTime() - Date.parse(since)) / 60_000));
  if (!Number.isFinite(minutes) || minutes < 1) return "방금";
  if (minutes < 60) return `${minutes}분째`;
  const hours = Math.floor(minutes / 60);
  return hours < 24 ? `${hours}시간째` : `${Math.floor(hours / 24)}일째`;
}

function reasonOf(item: NextItem, now: Date): string | undefined {
  const w = waited(item.since, now);
  switch (item.kind) {
    case "failed":
      return `${w} 멈춤 · 실패`;
    case "silent":
      return `${w} 응답 없음`;
    case "limit":
      return `한도에 걸려 ${w} 멈춤`;
    case "stopped":
      return `${w} 멈춤 · 중지됨`;
    case "answer":
      return `${w} 답을 기다림`;
    case "review":
      return `PR이 ${w} 검토를 기다림`;
    default:
      return undefined;
  }
}

/** The newest thing that happened in a run: its end, a card's last entry, or its start. */
function lastActivity(run: RunView): string {
  let latest = run.endedAt ?? run.startedAt;
  for (const card of run.cards) for (const at of [card.endedAt, card.log.at(-1)?.at]) if (at && at > latest) latest = at;
  for (const m of run.messages) if (m.at > latest) latest = m.at;
  return latest;
}

/**
 * What to pick up next across every project, without asking a model: stopped or
 * failed work oldest first, then questions waiting for an answer, then PRs waiting
 * for review (one pick per PR, however many cards reported it), then projects left idle for `idleDays` with cards still unfinished.
 * Each comes with a one-line reason ("3일째 멈춤 · 실패", "2시간째 답을 기다림").
 */
export function recommend(runs: RunView[], now: Date, handled: ReadonlySet<string> = new Set(), { idleDays = IDLE_DAYS, limit = 5 } = {}): Suggestion[] {
  const projectOf = new Map(runs.map((r) => [r.runId, r.project]));
  const { todo, stopped } = nextUp(runs, now, handled);
  const out: Suggestion[] = [];
  for (const item of [...stopped, ...todo]) {
    const reason = reasonOf(item, now);
    const project = projectOf.get(item.runId);
    // A note waits for no answer, so it is no "답을 기다림" pick.
    if (!reason || project === undefined || isNote(item)) continue;
    // Cards reporting the same PR are one review: the oldest ticket leads, the rest ride along.
    const same = item.kind === "review" ? out.find((s) => s.kind === "review" && s.item?.pr === item.pr) : undefined;
    if (same) {
      same.samePr = [...(same.samePr ?? []), item];
      same.reason = `${reasonOf(same.item!, now)} · 카드 ${same.samePr.length + 1}장`;
    } else out.push({ key: item.key, kind: item.kind as SuggestionKind, project, title: item.title, reason, since: item.since, item });
  }

  // Projects nobody has touched for days, with cards that never finished and no ticket already pointing at them.
  const ticketed = new Set([...stopped, ...todo].map((i) => projectOf.get(i.runId)));
  const byProject = new Map<string, RunView[]>();
  for (const run of runs) byProject.set(run.project, [...(byProject.get(run.project) ?? []), run]);
  for (const [project, list] of byProject) {
    if (ticketed.has(project)) continue;
    const latest = list.map(lastActivity).reduce((a, b) => (b > a ? b : a));
    const days = Math.floor((now.getTime() - Date.parse(latest)) / DAY_MS);
    const unfinished = list.flatMap((r) => r.cards).filter((c) => c.state !== "done").length;
    const key = `idle:${project}:${latest}`;
    if (days < idleDays || unfinished === 0 || handled.has(key)) continue;
    out.push({ key, kind: "idle", project, title: projectName(project), reason: `${days}일째 손대지 않음 · 끝나지 않은 카드 ${unfinished}장`, since: latest });
  }
  return out.sort((a, b) => TIER[a.kind] - TIER[b.kind] || a.since.localeCompare(b.since) || a.key.localeCompare(b.key)).slice(0, limit);
}

export interface ProjectNext {
  project: string;
  todo: NextItem[];
  stopped: NextItem[];
}

const URGENCY: Record<NextItem["kind"], number> = { failed: 0, silent: 0, limit: 0, stopped: 0, answer: 1, review: 2, check: 3, queued: 3 };

/**
 * Next up for every project at once (O runs and Claude Code sessions alike):
 * the projects whose most urgent ticket is most urgent first, then the one
 * that changed most recently.
 */
export function nextUpByProject(runs: RunView[], now: Date, handled: ReadonlySet<string> = new Set()): ProjectNext[] {
  const projectOf = new Map(runs.map((r) => [r.runId, r.project]));
  const { todo, stopped } = nextUp(runs, now, handled);
  const groups = new Map<string, ProjectNext>();
  const group = (item: NextItem) => {
    const project = projectOf.get(item.runId) ?? "";
    const found = groups.get(project) ?? { project, todo: [], stopped: [] };
    groups.set(project, found);
    return found;
  };
  for (const item of todo) group(item).todo.push(item);
  for (const item of stopped) group(item).stopped.push(item);
  const items = (g: ProjectNext) => [...g.todo, ...g.stopped];
  const rank = (g: ProjectNext) => Math.min(...items(g).map((i) => URGENCY[i.kind]));
  const newest = (g: ProjectNext) => items(g).reduce((a, i) => (i.since > a ? i.since : a), "");
  return [...groups.values()].sort((a, b) => rank(a) - rank(b) || newest(b).localeCompare(newest(a)));
}

interface FleetStats {
  running: number;
  stopped: number;
  prs: number;
  cardsToday: number;
  tokensToday: number;
}

/** The dashboard's strip: what is moving, what is not, and today's spend in cards and tokens. */
export function fleetStats(runs: RunView[], now: Date): FleetStats {
  const today = new Date(now);
  today.setHours(0, 0, 0, 0);
  const cards = runs.flatMap((r) => r.cards);
  const todays = runs.filter((r) => Date.parse(r.startedAt) >= today.getTime()).flatMap((r) => r.cards);
  return {
    running: cards.filter((c) => c.state === "running").length,
    stopped: cards.filter((c) => c.state === "failed" || c.state === "stopped").length + runs.filter(failedBeforeCards).length,
    prs: cards.filter((c) => prOf(c)).length,
    cardsToday: todays.length,
    tokensToday: todays.reduce((sum, c) => sum + tokensOf(c), 0),
  };
}

export interface ActivityItem {
  at: string;
  kind: "run" | "start" | "end" | "msg" | "pr";
  runId: string;
  cardId?: string;
  provider?: ProviderId;
  text: string;
  detail: string;
}

/** The newest happenings across runs: runs and cards starting and ending, team messages, PRs. */
export function activity(runs: RunView[], limit: number): ActivityItem[] {
  const items: ActivityItem[] = [];
  for (const run of runs) {
    items.push({ at: run.startedAt, kind: "run", runId: run.runId, text: run.title, detail: failedBeforeCards(run) ? `카드 시작 전 실패 · ${blockedReason(run)}` : `작업 시작 · 카드 ${run.cards.length}장` });
    for (const card of run.cards) {
      const who = { runId: run.runId, cardId: card.id, provider: card.provider };
      if (card.startedAt) items.push({ ...who, at: card.startedAt, kind: "start", text: card.label, detail: `시작 · ${run.title}` });
      if (card.endedAt) {
        const pr = prOf(card);
        items.push({ ...who, at: card.endedAt, kind: pr ? "pr" : "end", text: card.label, detail: pr ? `PR ${pr.replace(/^https:\/\/github\.com\//, "")}` : `${endWord(card.state as CardStatus)} · ${run.title}` });
      }
    }
    for (const m of run.messages) {
      const card = run.cards.find((c) => c.id === m.from);
      items.push({ at: m.at, kind: "msg", runId: run.runId, cardId: card?.id, provider: card?.provider, text: m.text, detail: `${card?.label ?? (m.from === "lead" ? "총괄" : m.from)} → ${m.to === "lead" ? "총괄" : m.to === "all" ? "모두" : (run.cards.find((c) => c.id === m.to)?.label ?? m.to)}` });
    }
  }
  return items.sort((a, b) => b.at.localeCompare(a.at)).slice(0, limit);
}
