import { Check, Copy, ExternalLink } from "lucide-react";
import { useCallback, useState } from "react";

import type { NextItem, StoppedKind, TodoKind } from "../../../src/core/attention.ts";
import { PROVIDER_SHORT, clip } from "../format.ts";
import { Rail, Stamp, TICKET_GHOST, TICKET_PRIMARY, Ticket, type StampTone } from "./Ticket.tsx";

export const STAMP: Record<TodoKind | StoppedKind, { label: string; tone: StampTone }> = {
  answer: { label: "답하기", tone: "rush" },
  review: { label: "PR 검토", tone: "info" },
  check: { label: "결과 확인", tone: "done" },
  queued: { label: "곧 시작", tone: "heat" },
  failed: { label: "실패", tone: "rush" },
  silent: { label: "응답 없음", tone: "rush" },
  limit: { label: "한도", tone: "heat" },
  stopped: { label: "중지됨", tone: "done" },
};

const HANDLED_KEY = "project-o-handled";
const HANDLED_MAX = 500;

function loadHandled(): Set<string> {
  try {
    const saved = JSON.parse(localStorage.getItem(HANDLED_KEY) ?? "[]");
    return new Set(Array.isArray(saved) ? saved.filter((k): k is string => typeof k === "string") : []);
  } catch {
    return new Set();
  }
}

/** Tickets a person has ticked off, kept in this browser (newest last, capped). */
export function useHandled() {
  const [handled, setHandled] = useState(loadHandled);
  const toggle = useCallback((key: string) => {
    setHandled((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      const kept = [...next].slice(-HANDLED_MAX);
      try {
        localStorage.setItem(HANDLED_KEY, JSON.stringify(kept));
      } catch {
        // Storage off: ticks last for this window only.
      }
      return new Set(kept);
    });
  }, []);
  return { handled, toggle };
}

/** The O command that acts on a ticket, for a person to paste into a terminal. */
function commandFor(item: NextItem): { label: string; text: string } | undefined {
  if (item.kind === "answer") return { label: "답장 명령", text: `o msg send --run ${item.runId} --as lead --to ${item.cardId} "답장 내용"` };
  if (item.kind === "silent") return { label: "중지 명령", text: `o stop ${item.runId}` };
  if (item.kind === "failed" || item.kind === "limit" || item.kind === "stopped") return { label: "상태 명령", text: `o status ${item.runId}` };
  return undefined;
}

const OPEN_LABEL: Partial<Record<NextItem["kind"], string>> = { check: "결과 보기", answer: "대화 보기" };

export function NextTicket({ item, now, onOpen, onDone, done }: { item: NextItem; now: Date; onOpen: (item: NextItem) => void; onDone?: (key: string) => void; done?: boolean }) {
  const [note, setNote] = useState<string>();
  // A note to the lead that asks nothing only needs reading, not an answer.
  const asks = item.kind !== "answer" || /[?？]|까요|할지|주세요|부탁|알려/.test(item.detail ?? "");
  const stamp = asks ? STAMP[item.kind] : { label: "읽기", tone: "heat" as const };
  const command = commandFor(item);
  const copy = (text: string, what: string) =>
    void navigator.clipboard.writeText(text).then(
      () => setNote(`${what} 복사함`),
      () => setNote("복사하지 못했습니다"),
    );
  return (
    <Ticket since={item.since} now={now} done={done} code={`${clip(item.runTitle, 28)}${item.provider ? ` · ${PROVIDER_SHORT[item.provider]}` : ""}`} stamp={<Stamp tone={stamp.tone}>{stamp.label}</Stamp>}>
      <div className="flex items-start gap-2">
        {onDone && (
          <button
            type="button"
            role="checkbox"
            aria-checked={Boolean(done)}
            aria-label={`${item.title} ${done ? "처리 취소" : "처리함으로 표시"}`}
            title={done ? "처리 취소" : "처리함"}
            onClick={() => onDone(item.key)}
            className="mt-0.5 grid size-4 shrink-0 place-items-center rounded-[3px] border-[1.5px] border-[var(--ink-dim)] text-[var(--paper)] aria-checked:bg-[var(--ink)] focus-visible:ring-2 focus-visible:ring-[var(--focus)] focus-visible:outline-none"
          >
            {done && <Check className="size-3" strokeWidth={3} aria-hidden="true" />}
          </button>
        )}
        <h4 className={`min-w-0 text-[13px] leading-snug font-semibold [overflow-wrap:anywhere] ${done ? "line-through decoration-[var(--ink-faint)]" : ""}`}>{item.title}</h4>
      </div>
      {item.detail && (
        <p className={`text-xs leading-relaxed [overflow-wrap:anywhere] ${item.kind === "answer" ? "rounded-md bg-[var(--paper-2)] px-2.5 py-1.5 text-[var(--ink)]" : "text-[var(--ink-dim)]"}`}>
          {clip(item.detail, 180)}
        </p>
      )}
      {!done && (
        <div className="mt-1 flex flex-wrap items-center gap-1">
          {item.pr ? (
            <a href={item.pr} target="_blank" rel="noreferrer" className={TICKET_PRIMARY}>
              <ExternalLink className="size-3.5" aria-hidden="true" />
              PR 열기
            </a>
          ) : (
            <button type="button" onClick={() => onOpen(item)} className={TICKET_PRIMARY}>
              {OPEN_LABEL[item.kind] ?? "기록 열기"}
            </button>
          )}
          {item.pr && (
            <button type="button" onClick={() => onOpen(item)} className={TICKET_GHOST}>
              기록 열기
            </button>
          )}
          {command && (
            <button type="button" onClick={() => copy(command.text, command.label)} title={command.text} className={TICKET_GHOST}>
              <Copy className="size-3.5" aria-hidden="true" />
              {command.label}
            </button>
          )}
          <span role="status" className="ml-auto text-2xs text-[var(--ink-faint)]">
            {note}
          </span>
        </div>
      )}
    </Ticket>
  );
}

/**
 * Next up, as in the Orchestrator app but cut to what an O project needs:
 * what to do next, and what stopped. Ticked tickets sink to the bottom for a
 * moment's undo and stay gone after that.
 */
export function NextUp({ todo, stopped, now, handled, onToggle, onOpen }: { todo: NextItem[]; stopped: NextItem[]; now: Date; handled: Set<string>; onToggle: (key: string) => void; onOpen: (item: NextItem) => void }) {
  // Ticked this visit: still shown (struck through) so a slip can be undone.
  const [ticked, setTicked] = useState<NextItem[]>([]);
  const tick = (item: NextItem) => {
    setTicked((t) => (t.some((x) => x.key === item.key) ? t.filter((x) => x.key !== item.key) : [...t, item]));
    onToggle(item.key);
  };
  const column = (items: NextItem[], kinds: string[]) => [
    ...items.map((item) => <NextTicket key={item.key} item={item} now={now} onOpen={onOpen} onDone={() => tick(item)} />),
    ...ticked.filter((t) => kinds.includes(t.kind) && handled.has(t.key)).map((item) => <NextTicket key={item.key} item={item} now={now} onOpen={onOpen} onDone={() => tick(item)} done />),
  ];
  return (
    <div className="@container">
      <div className="grid gap-5 @2xl:grid-cols-2">
        <Rail title="다음 할 일" count={todo.length} meta="답할 말 · 검토할 PR · 확인할 결과 · 곧 시작할 카드">
          {todo.length === 0 && ticked.length === 0 && <p className="text-xs text-muted-foreground">지금 할 일이 없습니다. 에이전트가 총괄에게 묻거나 PR을 올리면 여기 걸립니다.</p>}
          {column(todo, ["answer", "review", "check", "queued"])}
        </Rail>
        <Rail title="멈춘 작업" count={stopped.length} meta="실패 · 응답 없음 · 한도 · 중지">
          {stopped.length === 0 && <p className="text-xs text-muted-foreground">멈춘 작업이 없습니다.</p>}
          {column(stopped, ["failed", "silent", "limit", "stopped"])}
        </Rail>
      </div>
    </div>
  );
}
