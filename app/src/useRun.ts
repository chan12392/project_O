import { useCallback, useEffect, useMemo, useState } from "react";

import type { OEvent } from "../../src/core/events.ts";
import { snapshot, type RunView } from "../../src/core/snapshot.ts";
import demo from "./data/demo-run.json";

export interface Message {
  from: "me" | "lead";
  at: string;
  text: string;
}

export type ScreenState = "loading" | "empty" | "error" | "full";

const EVENTS = demo.events as OEvent[];
const TRANSCRIPT = demo.transcript as Message[];
/** The real run took six minutes; the preview replays it in about thirty seconds. */
const REPLAY_MS = 30_000;
const TICK_MS = 250;

const START = Date.parse(EVENTS[0].at);
const SPAN = Date.parse(EVENTS[EVENTS.length - 1].at) - START;

/** How many events had happened by a point in the replay (0 to 1). */
function countAt(fraction: number): number {
  const cutoff = START + SPAN * fraction;
  return EVENTS.filter((e) => Date.parse(e.at) <= cutoff).length;
}

/**
 * The browser preview's data source: today's recorded run, replayed.
 * The desktop build replaces this with a tail of the run's events.jsonl.
 */
export function useDemoRun(initial: number) {
  const [fraction, setFraction] = useState(initial);
  const playing = fraction < 1;

  useEffect(() => {
    if (!playing) return;
    const id = window.setInterval(() => setFraction((f) => Math.min(1, f + TICK_MS / REPLAY_MS)), TICK_MS);
    return () => window.clearInterval(id);
  }, [playing]);

  const visible = useMemo(() => EVENTS.slice(0, countAt(fraction)), [fraction]);
  const run: RunView | undefined = useMemo(() => snapshot(visible), [visible]);
  const messages = useMemo(() => TRANSCRIPT.filter((m) => Date.parse(m.at) <= START + SPAN * fraction), [fraction]);
  const replay = useCallback(() => setFraction(0), []);

  return { run, messages, events: visible, playing, replay };
}

/** Screen state, card selection and replay start live in the URL. */
export function readParams() {
  const params = new URLSearchParams(window.location.search);
  const state = params.get("state");
  const t = Number(params.get("t"));
  return {
    state: (["loading", "empty", "error"].includes(state ?? "") ? state : "full") as ScreenState,
    card: params.get("card") ?? undefined,
    start: Number.isFinite(t) && t > 0 ? Math.min(1, t) : 0,
  };
}

export function writeParam(key: string, value: string | undefined) {
  const url = new URL(window.location.href);
  if (value === undefined) url.searchParams.delete(key);
  else url.searchParams.set(key, value);
  window.history.replaceState(null, "", url);
}
