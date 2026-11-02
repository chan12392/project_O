import { useCallback, useEffect, useMemo, useState } from "react";

import type { OEvent } from "../../../src/core/events.ts";
import { snapshot, type RunView } from "../../../src/core/snapshot.ts";
import type { UsageWindow } from "../../../src/core/usage.ts";
import demo from "./demo-runs.json";
import demoSession from "./demo-session.json";

export interface Message {
  from: "me" | "lead";
  at: string;
  text: string;
}

export type ScreenState = "loading" | "empty" | "error" | "full";
/** The side panel's views, in tab order (shell/SidePanel.tsx draws each). */
export const PANEL_VIEWS = ["work", "office"] as const;
export type PanelView = (typeof PANEL_VIEWS)[number];

interface DemoRun {
  events: OEvent[];
  transcript: Message[];
}

/** Real runs from this machine, newest first (app/src/data/demo-runs.json), then a made-up Claude Code session (shown when Settings asks for sessions). */
const RUNS = [...demo.runs, ...demoSession.runs] as DemoRun[];
/** Subscription windows from `o usage` (Codex, Antigravity) at recording time; live mode reads usage.json. */
export const USAGE_WINDOWS = (demo.usage?.windows ?? []) as UsageWindow[];
/** A replayed run plays back in about thirty seconds, whatever it really took. */
const REPLAY_MS = 30_000;
const TICK_MS = 250;

function span(run: DemoRun): { start: number; length: number } {
  const start = Date.parse(run.events[0].at);
  return { start, length: Date.parse(run.events[run.events.length - 1].at) - start };
}

const IDS = RUNS.map((r) => (r.events[0].t === "run.start" ? r.events[0].runId : ""));
/** Each recorded run as it ended, drawn once; only the replayed one is redrawn as it plays. */
const ENDED = RUNS.map((run) => ({ view: snapshot(run.events) as RunView, messages: run.transcript }));
/** Recorded runs are old: once a replay ends, tickets age by the newest record. */
const NEWEST = new Date(Math.max(...RUNS.map((r) => Date.parse(r.events[r.events.length - 1].at))));

/**
 * The browser preview's data source: recorded runs, one of them replayed as if
 * live. Live runs come from useRunSource (useLiveRuns.ts); the replay only plays while `active`.
 */
export function useDemoRuns(initialRun: string | undefined, initialFraction: number, active = true) {
  const [replay, setReplay] = useState({ runId: initialRun && IDS.includes(initialRun) ? initialRun : IDS[0], fraction: initialFraction });
  const playing = replay.fraction < 1;

  useEffect(() => {
    if (!playing || !active) return;
    const id = window.setInterval(() => setReplay((r) => ({ ...r, fraction: Math.min(1, r.fraction + TICK_MS / REPLAY_MS) })), TICK_MS);
    return () => window.clearInterval(id);
  }, [playing, active]);

  const runs = useMemo(
    () =>
      RUNS.map((run, i) => {
        if (IDS[i] !== replay.runId || replay.fraction >= 1) return ENDED[i];
        const { start, length } = span(run);
        const cutoff = start + length * replay.fraction;
        return {
          view: snapshot(run.events.filter((e) => Date.parse(e.at) <= cutoff)) as RunView,
          messages: run.transcript.filter((m) => Date.parse(m.at) <= cutoff),
        };
      }),
    [replay],
  );
  const restart = useCallback((runId: string) => setReplay({ runId, fraction: 0 }), []);
  // Tickets age by the replay's moment while it plays.
  const clock = useMemo(() => {
    const replayed = RUNS[IDS.indexOf(replay.runId)];
    if (!playing || !replayed) return NEWEST;
    const { start, length } = span(replayed);
    return new Date(start + length * replay.fraction);
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
    view: view === "team" ? "office" : (PANEL_VIEWS.find((v) => v === view) ?? PANEL_VIEWS[0]),
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
