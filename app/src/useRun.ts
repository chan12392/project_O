import { useCallback, useEffect, useMemo, useState } from "react";

import type { OEvent } from "../../src/core/events.ts";
import { snapshot, type RunView } from "../../src/core/snapshot.ts";
import type { UsageWindow } from "../../src/core/usage.ts";
import demo from "./data/demo-runs.json";

export interface Message {
  from: "me" | "lead";
  at: string;
  text: string;
}

export type ScreenState = "loading" | "empty" | "error" | "full";
export type PanelView = "work" | "office";

interface DemoRun {
  events: OEvent[];
  transcript: Message[];
}

/** Real runs from this machine, newest first (app/src/data/demo-runs.json). */
const RUNS = demo.runs as DemoRun[];
/** Subscription windows from `o usage` (Codex, Antigravity) at recording time; live mode reads usage.json. */
export const USAGE_WINDOWS = (demo.usage?.windows ?? []) as UsageWindow[];
/** A replayed run plays back in about thirty seconds, whatever it really took. */
const REPLAY_MS = 30_000;
const TICK_MS = 250;

function span(run: DemoRun): { start: number; length: number } {
  const start = Date.parse(run.events[0].at);
  return { start, length: Date.parse(run.events[run.events.length - 1].at) - start };
}

/**
 * The browser preview's data source: recorded runs, one of them replayed as if
 * live. Live runs come from useRunSource (useLiveRuns.ts); the replay only plays while `active`.
 */
export function useDemoRuns(initialRun: string | undefined, initialFraction: number, active = true) {
  const ids = RUNS.map((r) => r.events[0].t === "run.start" ? r.events[0].runId : "");
  const [replay, setReplay] = useState({ runId: initialRun && ids.includes(initialRun) ? initialRun : ids[0], fraction: initialFraction });
  const playing = replay.fraction < 1;

  useEffect(() => {
    if (!playing || !active) return;
    const id = window.setInterval(() => setReplay((r) => ({ ...r, fraction: Math.min(1, r.fraction + TICK_MS / REPLAY_MS) })), TICK_MS);
    return () => window.clearInterval(id);
  }, [playing, active]);

  const runs = useMemo(
    () =>
      RUNS.map((run, i) => {
        const replayed = ids[i] === replay.runId;
        const { start, length } = span(run);
        const cutoff = replayed ? start + length * replay.fraction : Infinity;
        return {
          view: snapshot(run.events.filter((e) => Date.parse(e.at) <= cutoff)) as RunView,
          messages: run.transcript.filter((m) => Date.parse(m.at) <= cutoff),
        };
      }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [replay],
  );
  const restart = useCallback((runId: string) => setReplay({ runId, fraction: 0 }), []);
  // Recorded runs are old: tickets age by the replay's moment, or by the newest record once it ends.
  const clock = useMemo(() => {
    const replayed = RUNS[ids.indexOf(replay.runId)];
    if (playing && replayed) {
      const { start, length } = span(replayed);
      return new Date(start + length * replay.fraction);
    }
    return new Date(Math.max(...RUNS.map((r) => Date.parse(r.events[r.events.length - 1].at))));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [replay, playing]);
  return { runs, replayingId: playing ? replay.runId : undefined, restart, clock };
}

/** Screen state, run, panel view and card live in the URL. */
export function readParams() {
  const params = new URLSearchParams(window.location.search);
  const state = params.get("state");
  const view = params.get("view");
  const t = Number(params.get("t"));
  return {
    state: (["loading", "empty", "error"].includes(state ?? "") ? state : "full") as ScreenState,
    project: params.get("project") ?? undefined,
    run: params.get("run") ?? undefined,
    // The team chat now lives under the office; older links with view=team land there.
    view: (view === "office" || view === "team" ? "office" : "work") as PanelView,
    card: params.get("card") ?? undefined,
    start: Number.isFinite(t) && t > 0 ? Math.min(1, t) : 0,
    popout: params.get("popout") === "panel",
  };
}

export function writeParam(key: string, value: string | undefined) {
  const url = new URL(window.location.href);
  if (value === undefined) url.searchParams.delete(key);
  else url.searchParams.set(key, value);
  window.history.replaceState(null, "", url);
}
