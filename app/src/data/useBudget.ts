import { useSyncExternalStore } from "react";

import { backend, type BudgetReport } from "../platform/backend/index.ts";

/** The same answer App.tsx's backend gives: the recorded preview only when the link asks for it. */
const api = backend(new URLSearchParams(window.location.search).get("source") === "demo");
/** The budget changes as readings arrive (`o usage`, Claude's stream); it is read from saved snapshots only. */
const BUDGET_POLL_MS = 15_000;

/*
 * One report for the window: the usage panel's 오늘 rings and the status bar read the
 * same poll, so they never disagree and the machine is asked once per round.
 */
let report: BudgetReport | undefined;
let last = "";
let inflight = false;
let timer: number | undefined;
const listeners = new Set<() => void>();

function schedule() {
  window.clearTimeout(timer);
  if (listeners.size > 0 && !document.hidden) timer = window.setTimeout(poll, BUDGET_POLL_MS);
}

/** One read at a time: coming back to the window while a read is out does not start a second loop. */
async function poll() {
  if (inflight) return;
  inflight = true;
  window.clearTimeout(timer);
  try {
    const next = await api.budget();
    // The same report keeps the same state, so a quiet refresh re-renders nothing.
    const text = JSON.stringify(next);
    if (text !== last) {
      last = text;
      report = next;
      for (const listener of listeners) listener();
    }
  } catch {
    // An older `o` without `budget`, or no machine behind the page: readers keep what they show without it.
  } finally {
    inflight = false;
  }
  schedule();
}

const onVisibility = () => (document.hidden ? window.clearTimeout(timer) : void poll());

function subscribe(listener: () => void) {
  listeners.add(listener);
  if (listeners.size === 1) {
    document.addEventListener("visibilitychange", onVisibility);
    void poll();
  }
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0) {
      window.clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisibility);
    }
  };
}

/** Today's budget (ADR-0004) from saved readings, polled while the page is visible; undefined until the first answer or when unavailable. */
export function useBudget(): BudgetReport | undefined {
  return useSyncExternalStore(subscribe, () => report);
}
