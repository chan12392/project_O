import type { ProviderId } from "./plan.ts";
import type { CardView, RunView } from "./snapshot.ts";

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
}

/** A card still marked running with nothing new for this long has gone quiet. */
export const SILENT_MS = 10 * 60_000;
/** Finished runs stay on the list to be checked for this long. */
export const CHECK_WINDOW_MS = 24 * 60 * 60_000;

const PR_URL = /https:\/\/github\.com\/[\w.-]+\/[\w.-]+\/pull\/\d+/;

/** A pull request the card's agent reported, found in what it wrote. */
export function prOf(card: CardView): string | undefined {
  for (const text of [card.result ?? "", ...card.log.map((l) => l.text)]) {
    const match = PR_URL.exec(text);
    if (match) return match[0];
  }
  return undefined;
}

/** The last time a card showed any sign of life. */
export function lastSign(card: CardView): string | undefined {
  return card.log[card.log.length - 1]?.at ?? card.startedAt;
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

    // An agent wrote to the lead and the lead has not written back since.
    const lastLead = run.messages.filter((m) => m.from === "lead").at(-1)?.at ?? "";
    for (const m of run.messages) {
      if (m.to !== "lead" || m.at <= lastLead) continue;
      const card = cardOf.get(m.from);
      keep(todo, { ...base, key: `answer:${run.runId}:${m.from}:${m.at}`, kind: "answer", cardId: m.from, provider: card?.provider, title: card?.label ?? m.from, detail: m.text, since: m.at });
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
      const sign = lastSign(card);
      if (card.state === "running") {
        if (card.log.at(-1)?.kind === "limit") keep(stopped, { ...item, key: `limit:${run.runId}:${card.id}`, kind: "limit", detail: card.activity, since: card.log.at(-1)!.at });
        else if (sign && now.getTime() - Date.parse(sign) > SILENT_MS) keep(stopped, { ...item, key: `silent:${run.runId}:${card.id}:${sign}`, kind: "silent", detail: card.activity, since: sign });
      }
    }

    // A run that finished cleanly still wants a look at what it made.
    if (run.status === "done" && run.endedAt && now.getTime() - Date.parse(run.endedAt) < CHECK_WINDOW_MS) {
      keep(todo, { ...base, key: `check:${run.runId}`, kind: "check", title: run.title, detail: `카드 ${run.cards.length}장 완료`, since: run.endedAt });
    }
  }
  return { todo: todo.sort(byKindThenAge(TODO_ORDER)), stopped: stopped.sort(byKindThenAge(STOPPED_ORDER)) };
}

export interface FleetStats {
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
    stopped: cards.filter((c) => c.state === "failed" || c.state === "stopped").length,
    prs: cards.filter((c) => prOf(c)).length,
    cardsToday: todays.length,
    tokensToday: todays.reduce((sum, c) => sum + c.usage.input + c.usage.cached + c.usage.output, 0),
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
    items.push({ at: run.startedAt, kind: "run", runId: run.runId, text: run.title, detail: `작업 시작 · 카드 ${run.cards.length}장` });
    for (const card of run.cards) {
      const who = { runId: run.runId, cardId: card.id, provider: card.provider };
      if (card.startedAt) items.push({ ...who, at: card.startedAt, kind: "start", text: card.label, detail: `시작 · ${run.title}` });
      if (card.endedAt) {
        const pr = prOf(card);
        items.push({ ...who, at: card.endedAt, kind: pr ? "pr" : "end", text: card.label, detail: pr ? `PR ${pr.replace(/^https:\/\/github\.com\//, "")}` : `${card.state === "done" ? "완료" : card.state === "failed" ? "실패" : "중지"} · ${run.title}` });
      }
    }
    for (const m of run.messages) {
      const card = run.cards.find((c) => c.id === m.from);
      items.push({ at: m.at, kind: "msg", runId: run.runId, cardId: card?.id, provider: card?.provider, text: m.text, detail: `${card?.label ?? (m.from === "lead" ? "총괄" : m.from)} → ${m.to === "lead" ? "총괄" : m.to === "all" ? "모두" : (run.cards.find((c) => c.id === m.to)?.label ?? m.to)}` });
    }
  }
  return items.sort((a, b) => b.at.localeCompare(a.at)).slice(0, limit);
}
