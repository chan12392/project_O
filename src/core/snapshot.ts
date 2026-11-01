import type { CardStatus, OEvent } from "./events.ts";
import type { ProviderId } from "./plan.ts";

/**
 * Folds a run's event stream into what a screen shows. Pure: the CLI's
 * `o status` and the app both read runs through this, so they never disagree.
 */
export type CardState = "waiting" | "queued" | "running" | CardStatus;

export interface CardLogEntry {
  at: string;
  kind: "queued" | "tool" | "message" | "limit" | "end";
  text: string;
  /** For tool entries: the act in words and the raw detail the CLI reported. */
  tool?: { word: string; detail: string };
}

export interface CardView {
  id: string;
  provider: ProviderId;
  role: string;
  model?: string;
  effort?: string;
  state: CardState;
  /** The latest thing the card did, one line. */
  activity: string;
  result?: string;
  startedAt?: string;
  endedAt?: string;
  log: CardLogEntry[];
}

export interface RunView {
  runId: string;
  title: string;
  project: string;
  startedAt: string;
  endedAt?: string;
  status: "running" | "done" | "failed" | "stopped";
  cards: CardView[];
}

export function snapshot(events: OEvent[]): RunView | undefined {
  let run: RunView | undefined;
  const byId = new Map<string, CardView>();
  for (const e of events) {
    if (e.t === "run.start") {
      run = { runId: e.runId, title: e.title, project: e.project, startedAt: e.at, status: "running", cards: [] };
      for (const c of e.cards) {
        const card: CardView = { ...c, state: "waiting", activity: "", log: [] };
        byId.set(c.id, card);
        run.cards.push(card);
      }
      continue;
    }
    if (!run) continue;
    if (e.t === "run.end") {
      run.status = e.status;
      run.endedAt = e.at;
      continue;
    }
    const card = byId.get(e.cardId);
    if (!card) continue;
    switch (e.t) {
      case "card.queued":
        card.state = "queued";
        card.activity = e.reason;
        card.log.push({ at: e.at, kind: "queued", text: e.reason });
        break;
      case "card.start":
        card.state = "running";
        card.startedAt = e.at;
        break;
      case "card.tool":
        card.activity = describeTool(e.name, e.summary);
        card.log.push({ at: e.at, kind: "tool", text: `${e.name} ${e.summary}`.trim(), tool: { word: toolWord(e.name), detail: toolDetail(e.summary) } });
        break;
      case "card.message":
        card.activity = firstLine(e.text);
        card.log.push({ at: e.at, kind: "message", text: e.text });
        break;
      case "card.limit":
        card.activity = `사용량 한도: ${e.until}까지 차단`;
        card.log.push({ at: e.at, kind: "limit", text: card.activity });
        break;
      case "card.end":
        card.state = e.status;
        card.endedAt = e.at;
        card.result = e.result;
        if (e.result) card.activity = firstLine(e.result);
        card.log.push({ at: e.at, kind: "end", text: e.result ?? endWord(e.status) });
        break;
    }
  }
  return run;
}

/** Tool names as each CLI reports them, grouped into what a person would call the act. */
const TOOL_WORD: [RegExp, string][] = [
  [/^(shell|bash|run_command|powershell)$/i, "명령 실행"],
  [/^(read|read_file|view_file|grep|glob|list_dir|find_by_name)$/i, "파일 읽기"],
  [/^(edit|write|multiedit|write_file|replace_file_content|apply_patch|notebookedit)$/i, "파일 수정"],
  [/^(web_search|websearch|webfetch|search_web|read_url_content)$/i, "웹 검색"],
];

export function toolWord(name: string): string {
  return TOOL_WORD.find(([pattern]) => pattern.test(name))?.[1] ?? name;
}

function toolDetail(summary: string): string {
  const detail = summary.replace(/\\"/g, '"').trim();
  return /^".*"$/s.test(detail) ? detail.slice(1, -1) : detail;
}

/** "명령 실행: git status" rather than `shell "git status\"`; the log keeps the original. */
export function describeTool(name: string, summary: string): string {
  const detail = toolDetail(summary);
  return detail ? `${toolWord(name)}: ${detail}` : toolWord(name);
}

/** The first line, without the Markdown agents write in (`code`, **bold**): an activity is read at a glance. */
function firstLine(text: string): string {
  const line = text.split("\n").map((l) => l.trim()).find(Boolean) ?? "";
  return line.replace(/\*\*|`/g, "");
}

function endWord(status: CardStatus): string {
  return status === "done" ? "완료" : status === "stopped" ? "중지됨" : "실패";
}
