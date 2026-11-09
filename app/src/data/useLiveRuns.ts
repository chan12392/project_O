import { useCallback, useEffect, useState } from "react";

import type { OEvent } from "../../../src/core/events.ts";
import { foldRuns, type FoldedRun, type RunView } from "../../../src/core/snapshot.ts";
import { isSessionRun } from "../../../src/core/attention.ts";
import type { UsageWindow } from "../../../src/core/usage.ts";
import { sessionsShown, useSettings } from "./settings.ts";
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
/**
 * While the window is hidden or minimised the runs are still read, more slowly, so a card
 * that finishes meanwhile can be notified (Settings → 알림).
 */
const HIDDEN_POLL_MS = 15_000;
/** As many runs as `o runs --json` lists by default. */
const RUN_LIMIT = 50;

/**
 * The desktop app (project_O.exe) answers through Tauri commands with the same JSON text
 * the dev server's /api sends (`o runs --json`, `o usage --cached --json`, `o usage --json`).
 * With `sessions`, this PC's recent Claude Code sessions come along as runs (`o runs --json --sessions`).
 */
async function readRuns(sessions: boolean): Promise<{ runsText: string; usageText: string }> {
  const tauri = window.__TAURI_INTERNALS__;
  if (tauri) {
    const [runsText, usageText] = await Promise.all([
      tauri.invoke("runs", { limit: RUN_LIMIT, sessions }) as Promise<string>,
      (tauri.invoke("usage") as Promise<string>).catch(() => ""),
    ]);
    return { runsText, usageText };
  }
  const [runsRes, usageRes] = await Promise.all([fetch(sessions ? "/api/runs?sessions=1" : "/api/runs"), fetch("/api/usage")]);
  if (!runsRes.ok) throw new Error(`/api/runs ${runsRes.status}`);
  return { runsText: await runsRes.text(), usageText: usageRes.ok ? await usageRes.text() : "" };
}

/** Asks the official CLIs again; undefined when the reading failed. */
async function readFreshUsage(): Promise<UsageReading | undefined> {
  const tauri = window.__TAURI_INTERNALS__;
  if (tauri) return JSON.parse((await tauri.invoke("usage_refresh")) as string) as UsageReading;
  const res = await fetch("/api/usage/refresh", { method: "POST" });
  return res.ok ? ((await res.json()) as UsageReading) : undefined;
}

function requestedSource(): Source | undefined {
  // The desktop app shows only this machine's real runs: no recorded demo, not even as a fallback.
  if (window.__TAURI_INTERNALS__) return "live";
  const source = new URLSearchParams(window.location.search).get("source");
  return source === "live" || source === "demo" ? source : undefined;
}

/**
 * Runs and usage from project_O.exe's Tauri commands or the dev server's /api (app/dev/liveRuns.ts), polled every
 * 2 s while the page is visible and every 15 s while it is hidden. Lead chat threads come separately (`data/useLead.ts`), so run messages stay empty.
 * `runs` is undefined until the first answer; a failed first answer counts as no runs.
 */
function useLiveRuns(enabled: boolean, sessions: boolean) {
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
    // A run whose events did not change keeps its RunView and entry, so memoised readers and the
    // components showing it skip the poll. Content, not count: capped Claude Code sessions keep their length.
    let known = new Map<string, FoldedRun>();
    let entries = new Map<RunView, RunEntry>();
    const schedule = () => {
      window.clearTimeout(timer);
      if (!stopped) timer = window.setTimeout(poll, document.hidden ? HIDDEN_POLL_MS : POLL_MS);
    };
    const poll = async () => {
      if (inflight) return;
      inflight = true;
      try {
        const { runsText, usageText } = await readRuns(sessions);
        if (stopped) return;
        if (runsText !== lastRuns) {
          lastRuns = runsText;
          const body = JSON.parse(runsText) as { runs: { runId: string; events: OEvent[] }[] };
          known = foldRuns(body.runs, known);
          const nextEntries = new Map<RunView, RunEntry>();
          const list = [...known.values()].map(({ view }) => {
            const entry = entries.get(view) ?? { view, messages: [] };
            nextEntries.set(view, entry);
            return entry;
          });
          entries = nextEntries;
          setRuns((prev) => (prev && prev.length === list.length && prev.every((e, i) => e === list[i]) ? prev : list));
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
    // Back in view: read now rather than at the slow pace. Going hidden keeps the next read and slows the one after.
    const onVisibility = () => void (document.hidden || poll());
    void poll();
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      stopped = true;
      window.clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [enabled, sessions]);

  /** Asks the official CLIs again, the same reading `o usage` takes. */
  const refreshUsage = useCallback(async () => {
    setRefreshing(true);
    try {
      const fresh = await readFreshUsage();
      if (fresh) setUsage(fresh);
    } catch {
      // The status bar keeps the last reading and its time.
    } finally {
      setRefreshing(false);
    }
  }, []);

  return { runs, usage, failed, refreshing, refreshUsage };
}

/**
 * The screen's data: always live in the desktop app; in the browser `?source=live` or `?source=demo`, otherwise live when this
 * machine has at least one O run and demo when it has none (decided once).
 * Same shape as useDemoRuns, plus the usage windows and what live mode needs.
 */
export function useRunSource(initialRun: string | undefined, initialFraction: number) {
  const [source, setSource] = useState(requestedSource);
  const settings = useSettings();
  const live = useLiveRuns(source !== "demo", sessionsShown(settings, false));
  useEffect(() => {
    if (!source && live.runs) setSource(live.runs.length > 0 ? "live" : "demo");
  }, [source, live.runs]);
  const demo = useDemoRuns(initialRun, initialFraction, source === "demo");

  if (source === "demo") {
    // The demo's sample Claude Code session shows only once Settings → 일반 asks for sessions.
    const runs = sessionsShown(settings, true) ? demo.runs : demo.runs.filter((r) => !isSessionRun(r.view));
    return { pending: false, ...demo, runs, usageWindows: USAGE_WINDOWS, live: undefined };
  }
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
