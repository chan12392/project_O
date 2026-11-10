/**
 * Terminal agents (ADR-0042): a CLI without a chat protocol is shown as its
 * official interactive terminal, so the screen displays raw output and relays
 * keystrokes — the bytes are never parsed into chat messages.
 *
 * These are the contract shapes `o term *` prints and every backend (Tauri,
 * dev server, recorded demo) answers with, plus the pure bookkeeping the
 * terminal view and the composer share.
 */
import { canLead, LEAD_NAME, LEAD_PROVIDERS } from "./lead.ts";

/** `o agents status` rows, reduced to what the terminal rules need. */
export interface AgentStatusLike {
  id: string;
  installed: boolean;
  loggedIn: boolean | null;
}

/** One terminal session, as `o term list --json` and `o term open` return it. */
export interface TermSession {
  id: string;
  provider: string;
  project: string;
  /** "lead" for a lead tab's human-driven terminal, "card" for a card's terminal (ADR-0042 §6). */
  purpose: string;
  /** Absent in the CLI's own answers as JSON `null`; `undefined` is accepted too. */
  threadId?: string | null;
  runId?: string | null;
  cardId?: string | null;
  /** "running" | "awaiting-paste" (a task paste waits for human approval) | "exited". */
  state: string;
  exitCode: number | null;
  cols: number;
  rows: number;
  startedAt: string;
}

/** `o term read` answer: the output bytes since `offset`, and where the buffer now starts. */
export interface TermChunk {
  /** The read position to pass next. */
  offset: number;
  /** The new output bytes, base64. */
  data: string;
  state: string;
  exitCode: number | null;
  /**
   * The oldest byte still kept. The host drops old output past a cap, so a caller
   * positioned before `start` missed bytes and must reset its screen.
   */
  start: number;
}

/** The session a lead thread's terminal shows: the newest live one, else the newest (ADR-0042 §6). */
export function sessionForThread(sessions: readonly TermSession[] | undefined, threadId: string | undefined): TermSession | undefined {
  if (!threadId || !sessions) return undefined;
  const mine = sessions.filter((s) => s.purpose === "lead" && s.threadId === threadId);
  return mine.find((s) => s.state !== "exited") ?? mine.at(-1);
}

/** The session a card's terminal shows: the newest live one for that card, else the newest. */
export function sessionForCard(sessions: readonly TermSession[] | undefined, runId: string, cardId: string): TermSession | undefined {
  const mine = sessions?.filter((s) => s.purpose === "card" && s.runId === runId && s.cardId === cardId) ?? [];
  return mine.find((s) => s.state !== "exited") ?? mine.at(-1);
}

/**
 * The read position after one `termRead` answer. Bytes before `start` were dropped,
 * so a position below it is stale: the caller resets its screen and continues from
 * the new data (a risen `start`, ADR-0042 §6).
 */
export function nextOffset(offset: number, chunk: Pick<TermChunk, "offset" | "start">): { offset: number; stale: boolean } {
  return { offset: chunk.offset, stale: offset < chunk.start };
}

/** base64 → bytes, as `o term read` sends them. `atob`/`btoa` exist in the window and in Node ≥16. */
export function b64ToBytes(data: string): Uint8Array {
  const bin = atob(data);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return bytes;
}

/** bytes → base64, as `o term write` takes them. */
export function bytesToB64(bytes: Uint8Array): string {
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin);
}

/** The largest payload one `termWrite` sends (raw bytes before base64): a pasted burst goes over as pieces. */
export const TERM_WRITE_CHUNK = 32 * 1024;

/**
 * Split input for `o term write`: pieces of at most TERM_WRITE_CHUNK bytes that
 * never break a UTF-8 sequence, so a long paste reaches the terminal intact and
 * the dev server's body limit accepts every piece.
 */
export function termWriteChunks(bytes: Uint8Array): Uint8Array[] {
  const pieces: Uint8Array[] = [];
  for (let at = 0; at < bytes.length; ) {
    let end = Math.min(at + TERM_WRITE_CHUNK, bytes.length);
    if (end < bytes.length) {
      // A cut inside a multi-byte sequence would corrupt it: back off to the
      // sequence's lead byte (continuations are 0b10xxxxxx). If the window is
      // all continuations the input is corrupt — keep the hard cut.
      let lead = end;
      while (lead > at && (bytes[lead] & 0xc0) === 0x80) lead--;
      if (lead > at) end = lead;
    }
    pieces.push(bytes.slice(at, end));
    at = end;
  }
  return pieces;
}

/** Shown wherever the dev backend cannot do a human-only terminal or agy operation (ADR-0042). */
export const TERM_DESKTOP_ONLY = "개발 서버에서는 할 수 없습니다. 데스크톱 앱이나 직접 실행한 o 명령을 쓰세요.";

/** A session's input FIFO: `send` enqueues, `reset` clears a broken state, `pending` counts in-flight sends. */
export interface TermWriteQueue {
  send: (dataBase64: string) => Promise<void>;
  /** Clears the broken state so new input flows again; input dropped by the failure is never re-sent. */
  reset: () => void;
  /** Sends still in flight — the queue is retired only once it has drained. */
  pending: () => number;
}

/**
 * One write at a time per session (ADR-0042): `termWrite` calls queue behind each
 * other so keystrokes reach the terminal in typing order — parallel requests
 * would create out-of-order queue files. Once a write fails the queue breaks:
 * the failure is checked again right before each send, so input already queued
 * behind it is rejected too, and anything enqueued later rejects — until the
 * person resets. Dropped input is never silently sent or re-sent.
 */
export function termWriteQueue(send: (dataBase64: string) => Promise<void>, onIdle?: () => void): TermWriteQueue {
  let tail: Promise<void> = Promise.resolve();
  let broken: unknown;
  let broke = false;
  // Raised by every failure: input enqueued before it belongs to an older epoch and is
  // dropped even when the person resets before the queue has reached it.
  let epoch = 0;
  let pending = 0;
  return {
    send(dataBase64) {
      if (broke) return Promise.reject(broken);
      pending++;
      const mine = epoch;
      const next = tail
        .then(() => {
          if (broke) throw broken;
          if (mine !== epoch) throw new Error("앞선 입력이 실패해 보내지 않았습니다");
          return send(dataBase64);
        })
        .finally(() => {
          pending--;
          if (pending === 0) onIdle?.();
        });
      tail = next.catch((e: unknown) => {
        // A send dropped for its epoch is not a new failure: the queue may already be reset.
        if (mine !== epoch) return;
        epoch++;
        broke = true;
        broken = e;
      });
      return next;
    },
    reset() {
      // The tail is kept: a second view resetting an already recovered queue must
      // not let its input overtake a send the first view has in flight.
      broke = false;
      broken = undefined;
    },
    pending: () => pending,
  };
}

const sessionQueues = new Map<string, TermWriteQueue>();
/** Sessions that ended with sends still in flight: retired when the queue drains. */
const endedQueues = new Set<string>();

/**
 * The one input queue for a session (ADR-0042): every view of the same session —
 * a remount or two panes — shares it, so input can never overtake a slower view's
 * pending write. The first view's transport is kept.
 */
export function termWriteQueueFor(sessionId: string, send: (dataBase64: string) => Promise<void>): TermWriteQueue {
  let queue = sessionQueues.get(sessionId);
  if (!queue) {
    queue = termWriteQueue(send, () => {
      if (endedQueues.delete(sessionId)) sessionQueues.delete(sessionId);
    });
    sessionQueues.set(sessionId, queue);
  }
  return queue;
}

/** Retire a session's queue once the session ended: now, or when what is still in flight has drained. */
export function termWriteQueueEnd(sessionId: string): void {
  const queue = sessionQueues.get(sessionId);
  if (!queue) return;
  if (queue.pending() === 0) sessionQueues.delete(sessionId);
  else endedQueues.add(sessionId);
}

/** The composer's agent menu (ADR-0042): chat leads that can lead now, Antigravity as a terminal lead once installed, Devin not yet. */
export function composerAgentChoices(agents: readonly AgentStatusLike[]): { value: string; label: string; disabled: boolean; hint?: string }[] {
  const usable = agents.filter(canLead).map((a) => a.id);
  const status = (id: string) => agents.find((a) => a.id === id);
  const hint = (id: string): string | undefined => {
    const s = status(id);
    if (!s?.installed) return "설치되지 않았습니다 · 설정 → 에이전트";
    if (s.loggedIn === false) return "로그인이 필요합니다 · 설정 → 에이전트";
    if (s.loggedIn !== true) return "로그인 상태를 CLI가 알려 주지 않습니다";
    return undefined;
  };
  return [
    ...LEAD_PROVIDERS.map((id) =>
      id === "agy"
        ? {
            value: id,
            label: `${LEAD_NAME[id]} · 터미널`,
            disabled: !status(id)?.installed,
            hint: status(id)?.installed ? "공식 터미널 화면에서 직접 대화합니다" : hint(id),
          }
        : { value: id, label: LEAD_NAME[id], disabled: !usable.includes(id), hint: hint(id) },
    ),
    { value: "devin", label: "Devin", disabled: true, hint: "총괄은 아직 지원하지 않음" },
  ];
}

/** The Antigravity settings a report keeps, beside the always-shown `notice`. */
export interface AgyState {
  workers: boolean;
  acknowledgedAt: string | null;
  confirmPaste: boolean;
}

/**
 * One `o agy` change (ADR-0042): enabling workers needs `ack: true` in the same
 * call; turning workers off forgets the acknowledgement so enabling asks again.
 * Returns the new state, or the refusal message.
 */
export function agyApply(state: AgyState, change: { workers?: boolean; ack?: boolean; confirmPaste?: boolean }, now: string): AgyState | string {
  if (change.workers === true && change.ack !== true) return "안내와 약관을 읽고 동의 확인을 해 주세요.";
  const next = { ...state };
  if (change.workers !== undefined) {
    next.workers = change.workers;
    next.acknowledgedAt = change.workers ? now : null;
  }
  if (change.confirmPaste !== undefined) next.confirmPaste = change.confirmPaste;
  return next;
}

/** The plans folder a lead terminal watches: `<home-as-X>-lead/<threadId>/plans/` (ADR-0042 §6). */
export function plansFolder(home: string, threadId: string): string {
  const root = home.replace(/[\\/]+$/, "").replace(/[^\\/]+$/, (name) => `${name}-lead`);
  return `${root}/${threadId}/plans/`;
}

/**
 * The note the person pastes into a lead terminal themselves so the agent's plan
 * reaches O: write the plan JSON into the plans folder, O checks it and shows it
 * as a plan card (ADR-0042 §6). O never types into the terminal.
 */
export function planGuideText(plansDir: string): string {
  return [
    `계획을 JSON 파일로 ${plansDir} 아래에 쓰세요.`,
    `모양: {"title": "계획 제목", "cards": [{"id": "작업 id", "provider": "claude|codex|agy|opencode|devin", "task": "할 일", "role?": "역할", "model?": "모델", "effort?": "노력 수준"}]}.`,
    "파일은 먼저 다른 이름(예: plan.tmp)으로 완성한 뒤 <이름>.json으로 이름을 바꾸세요. 고친 계획은 같은 이름을 다시 써도 됩니다.",
    "O가 정책으로 검사해 이 대화의 계획 카드로 보여 줍니다. 실행은 사람이 누릅니다.",
  ].join("\n");
}
