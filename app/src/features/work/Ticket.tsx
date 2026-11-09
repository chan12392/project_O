import { type ReactNode } from "react";

/*
 * Thermal-paper tickets, after the Orchestrator app's ticket rail (its DESIGN.md:
 * "work that waits on you hangs as thermal paper and ages in heat"). The top edge
 * darkens with age and means nothing else; stamps say what kind of wait it is.
 */
type Heat = "fresh" | "warm" | "hot" | "scorched";

function heatOf(since: string, now: Date): Heat {
  const minutes = (now.getTime() - Date.parse(since)) / 60_000;
  if (!Number.isFinite(minutes) || minutes < 15) return "fresh";
  if (minutes < 60) return "warm";
  return minutes < 24 * 60 ? "hot" : "scorched";
}

/** How long it has waited, in the shortest honest form. */
export function age(since: string, now: Date): string {
  const minutes = Math.max(0, Math.floor((now.getTime() - Date.parse(since)) / 60_000));
  if (minutes < 1) return "방금";
  if (minutes < 60) return `${minutes}분`;
  const hours = Math.floor(minutes / 60);
  return hours < 24 ? `${hours}시간` : `${Math.floor(hours / 24)}일`;
}

export type StampTone = "rush" | "heat" | "info" | "done";

export function Stamp({ tone, children }: { tone: StampTone; children: ReactNode }) {
  return <span className={`stamp stamp--${tone}`}>{children}</span>;
}

export function Ticket({ since, now, code, stamp, children, done }: { since: string; now: Date; code: ReactNode; stamp?: ReactNode; children: ReactNode; done?: boolean }) {
  return (
    <article className="ticket flex min-w-0 flex-col gap-1.5 px-3.5 pt-3 pb-3" data-heat={done ? "fresh" : heatOf(since, now)} data-done={done || undefined}>
      <header className="flex min-w-0 items-center gap-2">
        <span className="truncate text-2xs text-[var(--ink-faint)]">{code}</span>
        {stamp}
        <time dateTime={since} title={new Date(since).toLocaleString("ko-KR")} className="ml-auto shrink-0 font-mono text-2xs font-semibold text-[var(--ink-dim)] tabular-nums">
          {age(since, now)}
        </time>
      </header>
      {children}
    </article>
  );
}

const ACTION = "inline-flex h-7 items-center gap-1.5 rounded-md px-2.5 text-xs font-semibold focus-visible:ring-2 focus-visible:ring-[var(--focus)] focus-visible:outline-none";
export const TICKET_PRIMARY = `${ACTION} bg-[var(--ink)] text-[var(--paper)] hover:bg-[var(--ink-dim)]`;
export const TICKET_GHOST = `${ACTION} text-[var(--ink-dim)] hover:bg-[var(--paper-2)] hover:text-[var(--ink)]`;
