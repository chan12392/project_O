import { Check, ChevronRight, Copy, ExternalLink } from "lucide-react";
import { useState, useSyncExternalStore, type ReactNode } from "react";

import { isNote, isSessionRun, needsAction, ticketCommand, type NextItem, type StoppedKind, type TodoKind } from "../../../../src/core/attention.ts";
import { PROVIDER_SHORT, clip } from "../../lib/format.ts";
import { readStored, writeStored } from "../../lib/storage.ts";
import { ExternalAnchor } from "../../ui/ExternalAnchor.tsx";
import { Rail, Stamp, TICKET_GHOST, TICKET_PRIMARY, Ticket, type StampTone } from "./Ticket.tsx";

export const STAMP: Record<TodoKind | StoppedKind, { label: string; tone: StampTone }> = {
  answer: { label: "답하기", tone: "rush" },
  review: { label: "사람 검토 필요", tone: "info" },
  check: { label: "결과 확인", tone: "done" },
  queued: { label: "곧 시작", tone: "heat" },
  failed: { label: "실패", tone: "rush" },
  silent: { label: "응답 없음", tone: "rush" },
  limit: { label: "한도", tone: "heat" },
  stopped: { label: "중지됨", tone: "done" },
};

const HANDLED_KEY = "project-o-handled";
const HANDLED_MAX = 500;

const loadHandled = () =>
  readStored(HANDLED_KEY, new Set<string>(), (saved) => (Array.isArray(saved) ? new Set(saved.filter((k): k is string => typeof k === "string")) : undefined));

/*
 * Ticked tickets are one store for the window: the project's Next up, the
 * all-projects view and the sidebar's count tick off the same set.
 */
let handledNow = loadHandled();
const handledListeners = new Set<() => void>();

function toggleHandled(key: string) {
  const next = new Set(handledNow);
  if (next.has(key)) next.delete(key);
  else next.add(key);
  handledNow = new Set([...next].slice(-HANDLED_MAX));
  writeStored(HANDLED_KEY, [...handledNow]);
  for (const listener of handledListeners) listener();
}

function subscribeHandled(listener: () => void) {
  handledListeners.add(listener);
  return () => void handledListeners.delete(listener);
}

/** Tickets a person has ticked off, kept in this browser (newest last, capped). */
export function useHandled() {
  const handled = useSyncExternalStore(subscribeHandled, () => handledNow);
  return { handled, toggle: toggleHandled };
}


const OPEN_LABEL: Partial<Record<NextItem["kind"], string>> = { check: "결과 보기", answer: "대화 보기" };

/** The tick that marks a ticket handled; every ticket (and recommendation) uses this one control. */
export function DoneBox({ title, done, onToggle }: { title: string; done?: boolean; onToggle: () => void }) {
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={Boolean(done)}
      aria-label={`${title} ${done ? "처리 취소" : "처리함으로 표시"}`}
      title={done ? "처리 취소" : "처리함"}
      onClick={onToggle}
      className="mt-0.5 grid size-4 shrink-0 place-items-center rounded-[3px] border-[1.5px] border-[var(--ink-dim)] text-[var(--paper)] aria-checked:bg-[var(--ink)] focus-visible:ring-2 focus-visible:ring-[var(--focus)] focus-visible:outline-none"
    >
      {done && <Check className="size-3" strokeWidth={3} aria-hidden="true" />}
    </button>
  );
}

export function NextTicket({ item, now, onOpen, onDone, done }: { item: NextItem; now: Date; onOpen: (item: NextItem) => void; onDone?: (key: string) => void; done?: boolean }) {
  const [note, setNote] = useState<string>();
  // A note to the lead that asks nothing only needs reading, not an answer.
  const stamp = isNote(item) ? { label: "읽기", tone: "heat" as const } : STAMP[item.kind];
  const command = ticketCommand(item);
  const copy = (text: string, what: string) =>
    void navigator.clipboard.writeText(text).then(
      () => setNote(`${what} 복사함`),
      () => setNote("복사하지 못했습니다"),
    );
  return (
    <Ticket since={item.since} now={now} done={done} code={`${clip(item.runTitle, 28)}${item.provider ? ` · ${PROVIDER_SHORT[item.provider]}` : ""}`} stamp={<Stamp tone={stamp.tone}>{stamp.label}</Stamp>}>
      <div className="flex items-start gap-2">
        {onDone && <DoneBox title={item.title} done={done} onToggle={() => onDone(item.key)} />}
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
            <ExternalAnchor href={item.pr} className={TICKET_PRIMARY}>
              <ExternalLink className="size-3.5" aria-hidden="true" />
              PR 열기
            </ExternalAnchor>
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
            <button
              type="button"
              onClick={() => copy(command.text, command.label.replace(/ 복사$/, ""))}
              title={`앱에서는 아직 할 수 없어 터미널에 붙여 넣을 명령을 복사합니다\n${command.text}`}
              className={TICKET_GHOST}
            >
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
 * A list of tickets in the order a person works them: what waits on them first,
 * then notes that only need reading, then a Claude Code session's read-only
 * tickets, folded. `ticket` draws one ticket; `ticked` are the struck-through
 * ones kept for an undo; `grid` lays the folded groups out like the list.
 */
export function TicketGroups({ items, ticked = [], ticket, grid = "" }: { items: NextItem[]; ticked?: NextItem[]; ticket: (item: NextItem, done?: boolean) => ReactNode; grid?: string }) {
  const act = items.filter(needsAction);
  const notes = items.filter((i) => isNote(i) && !isSessionRun(i));
  const sessions = items.filter(isSessionRun);
  return (
    <>
      {act.map((item) => ticket(item))}
      {ticked.map((item) => ticket(item, true))}
      {notes.length > 0 && (
        <Fold open title="읽기만 하면 되는 메모" count={notes.length} grid={grid}>
          {notes.map((item) => ticket(item))}
        </Fold>
      )}
      {sessions.length > 0 && (
        <Fold title="Claude Code 세션 · 읽기 전용" count={sessions.length} grid={grid}>
          {sessions.map((item) => ticket(item))}
        </Fold>
      )}
    </>
  );
}

function Fold({ title, count, open, grid, children }: { title: string; count: number; open?: boolean; grid: string; children: ReactNode }) {
  return (
    <details open={open} className={`group min-w-0 ${grid ? "col-span-full" : ""}`}>
      <summary className="flex cursor-pointer list-none items-center gap-1.5 rounded-sm py-0.5 text-2xs font-semibold text-muted-foreground hover:text-foreground focus-ring [&::-webkit-details-marker]:hidden">
        <ChevronRight className="size-3.5 transition-transform group-open:rotate-90 motion-reduce:transition-none" aria-hidden="true" />
        {title}
        <span className="tabular-nums">{count}</span>
      </summary>
      <div className={`mt-2 ${grid || "flex flex-col gap-2.5"}`}>{children}</div>
    </details>
  );
}

/** How many tickets wait on a person: what a badge or a rail's count shows. */
export function actionCount(items: NextItem[]): number {
  return items.filter(needsAction).length;
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
  const ticket = (item: NextItem, done?: boolean) => <NextTicket key={item.key} item={item} now={now} onOpen={onOpen} onDone={() => tick(item)} done={done} />;
  const struck = (kinds: string[]) => ticked.filter((t) => kinds.includes(t.kind) && handled.has(t.key));
  const todoStruck = struck(["answer", "review", "check", "queued"]);
  return (
    <div className="@container">
      <div className="grid gap-5 @2xl:grid-cols-2">
        <Rail title="다음 할 일" count={actionCount(todo)} meta="답할 말 · 검토할 PR · 확인할 결과 · 곧 시작할 카드">
          {actionCount(todo) === 0 && todoStruck.length === 0 && <p className="text-xs text-muted-foreground">지금 할 일이 없습니다. 에이전트가 총괄에게 묻거나 PR을 올리면 여기 걸립니다.</p>}
          <TicketGroups items={todo} ticked={todoStruck} ticket={ticket} />
        </Rail>
        <Rail title="멈춘 작업" count={actionCount(stopped)} meta="실패 · 응답 없음 · 한도 · 중지">
          {actionCount(stopped) === 0 && <p className="text-xs text-muted-foreground">멈춘 작업이 없습니다.</p>}
          <TicketGroups items={stopped} ticked={struck(["failed", "silent", "limit", "stopped"])} ticket={ticket} />
        </Rail>
      </div>
    </div>
  );
}
