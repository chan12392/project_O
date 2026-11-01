import { useCallback, useEffect, useState } from "react";

import type { OEvent } from "../../src/core/events.ts";
import { snapshot, type RunView } from "../../src/core/snapshot.ts";
import type { UsageWindow } from "../../src/core/usage.ts";
import { USAGE_WINDOWS, useDemoRuns, type Message } from "./useRun.ts";

/** Where the screen's runs come from: this machine's O home, or the recorded demo. */
type Source = "live" | "demo";

interface RunEntry {
  view: RunView;
  messages: Message[];
}

/** What `o usage` saved (usage.json); `at` is empty when it never ran. */
interface UsageReading {
  at: string;
  windows: UsageWindow[];
  unavailable: { provider: string; reason: string }[];
}

const POLL_MS = 2_000;

function requestedSource(): Source | undefined {
  const source = new URLSearchParams(window.location.search).get("source");
  return source === "live" || source === "demo" ? source : undefined;
}

/**
 * Runs and usage from the dev server's /api (app/dev/liveRuns.ts), polled while
 * the page is visible. Lead chat transcripts are not stored by O yet, so messages stay empty.
 * `runs` is undefined until the first answer; a failed first answer counts as no runs.
 */
function useLiveRuns(enabled: boolean) {
  const [runs, setRuns] = useState<RunEntry[]>();
  const [usage, setUsage] = useState<UsageReading>();
  const [failed, setFailed] = useState(false);
  const [refreshing, setRefreshing] = useState(false);

  useEffect(() => {
    if (!enabled) return;
    let stopped = false;
    let inflight = false;
    let timer: number | undefined;
    // Unchanged answers keep the same objects, so a quiet poll re-renders nothing.
    let lastRuns = "";
    let lastUsage = "";
    // Events are append-only: a run whose event count did not change keeps its RunView,
    // so memoised readers and the components showing it skip the poll.
    let known = new Map<string, { count: number; entry: RunEntry }>();
    const schedule = () => {
      window.clearTimeout(timer);
      if (!stopped && !document.hidden) timer = window.setTimeout(poll, POLL_MS);
    };
    const poll = async () => {
      if (inflight) return;
      inflight = true;
      try {
        const [runsRes, usageRes] = await Promise.all([fetch("/api/runs"), fetch("/api/usage")]);
        if (!runsRes.ok) throw new Error(`/api/runs ${runsRes.status}`);
        const runsText = await runsRes.text();
        const usageText = usageRes.ok ? await usageRes.text() : "";
        if (stopped) return;
        if (runsText !== lastRuns) {
          lastRuns = runsText;
          const body = JSON.parse(runsText) as { runs: { runId: string; events: OEvent[] }[] };
          const next = new Map<string, { count: number; entry: RunEntry }>();
          const entries = body.runs.flatMap((r) => {
            const hit = known.get(r.runId);
            if (hit && hit.count === r.events.length) {
              next.set(r.runId, hit);
              return [hit.entry];
            }
            const view = snapshot(r.events);
            if (!view) return [];
            const entry = { view, messages: [] };
            next.set(r.runId, { count: r.events.length, entry });
            return [entry];
          });
          known = next;
          setRuns((prev) => (prev && prev.length === entries.length && prev.every((e, i) => e === entries[i]) ? prev : entries));
        }
        if (usageText && usageText !== lastUsage) {
          lastUsage = usageText;
          setUsage(JSON.parse(usageText) as UsageReading);
        }
        setFailed(false);
      } catch {
        if (stopped) return;
        setFailed(true);
        setRuns((r) => r ?? []);
      } finally {
        inflight = false;
        schedule();
      }
    };
    const onVisibility = () => (document.hidden ? window.clearTimeout(timer) : void poll());
    void poll();
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      stopped = true;
      window.clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [enabled]);

  /** Asks the official CLIs again, the same reading `o usage` takes. */
  const refreshUsage = useCallback(async () => {
    setRefreshing(true);
    try {
      const res = await fetch("/api/usage/refresh", { method: "POST" });
      if (res.ok) setUsage((await res.json()) as UsageReading);
    } catch {
      // The status bar keeps the last reading and its time.
    } finally {
      setRefreshing(false);
    }
  }, []);

  return { runs, usage, failed, refreshing, refreshUsage };
}

/**
 * The screen's data: `?source=live` or `?source=demo`, otherwise live when this
 * machine has at least one O run and demo when it has none (decided once).
 * Same shape as useDemoRuns, plus the usage windows and what live mode needs.
 */
export function useRunSource(initialRun: string | undefined, initialFraction: number) {
  const [source, setSource] = useState(requestedSource);
  const live = useLiveRuns(source !== "demo");
  useEffect(() => {
    if (!source && live.runs) setSource(live.runs.length > 0 ? "live" : "demo");
  }, [source, live.runs]);
  const demo = useDemoRuns(initialRun, initialFraction, source === "demo");

  if (source === "demo") return { pending: false, ...demo, usageWindows: USAGE_WINDOWS, live: undefined };
  return {
    /** Waiting for the first answer before deciding or showing anything. */
    pending: !live.runs,
    runs: live.runs ?? [],
    replayingId: undefined,
    /** Nothing to replay; the status bar offers it only for the demo. */
    restart: (_runId: string) => {},
    /** Live tickets age by the wall clock. */
    clock: undefined,
    usageWindows: live.usage?.windows ?? [],
    live: { failed: live.failed, usage: live.usage, refreshing: live.refreshing, refreshUsage: live.refreshUsage },
  };
}

export type LiveStatus = NonNullable<ReturnType<typeof useRunSource>["live"]>;
