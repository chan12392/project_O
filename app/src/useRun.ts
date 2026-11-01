import { useCallback, useEffect, useMemo, useState } from "react";

import type { OEvent } from "../../src/core/events.ts";
import { snapshot, type RunView } from "../../src/core/snapshot.ts";
import demo from "./data/demo-runs.json";

export interface Message {
  from: "me" | "lead";
  at: string;
  text: string;
}

export type ScreenState = "loading" | "empty" | "error" | "full";
export type PanelView = "work" | "office" | "team";

interface DemoRun {
  events: OEvent[];
  transcript: Message[];
}

/** Real runs from this machine, newest first (app/src/data/demo-runs.json). */
const RUNS = demo.runs as DemoRun[];
/** A replayed run plays back in about thirty seconds, whatever it really took. */
const REPLAY_MS = 30_000;
const TICK_MS = 250;

function span(run: DemoRun): { start: number; length: number } {
  const start = Date.parse(run.events[0].at);
  return { start, length: Date.parse(run.events[run.events.length - 1].at) - start };
}

/**
 * The browser preview's data source: recorded runs, one of them replayed as if
 * live. The desktop build replaces this with the runs folder and a tail of events.jsonl.
 */
export function useDemoRuns(initialRun: string | undefined, initialFraction: number) {
  const ids = RUNS.map((r) => r.events[0].t === "run.start" ? r.events[0].runId : "");
  const [replay, setReplay] = useState({ runId: initialRun && ids.includes(initialRun) ? initialRun : ids[0], fraction: initialFraction });
  const playing = replay.fraction < 1;

  useEffect(() => {
    if (!playing) return;
    const id = window.setInterval(() => setReplay((r) => ({ ...r, fraction: Math.min(1, r.fraction + TICK_MS / REPLAY_MS) })), TICK_MS);
    return () => window.clearInterval(id);
  }, [playing]);

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
  return { runs, replayingId: playing ? replay.runId : undefined, restart };
}

/** Screen state, run, panel view and card live in the URL. */
export function readParams() {
  const params = new URLSearchParams(window.location.search);
  const state = params.get("state");
  const view = params.get("view");
  const t = Number(params.get("t"));
  return {
    state: (["loading", "empty", "error"].includes(state ?? "") ? state : "full") as ScreenState,
    run: params.get("run") ?? undefined,
    view: (["work", "office", "team"].includes(view ?? "") ? view : "work") as PanelView,
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
