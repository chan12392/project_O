import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";

import type { PanelView } from "./useRun.ts";

const KEY = "project-o.panel";
const CHANNEL = "project-o-panel";
const MIN = 320;
const MIN_CHAT = 360;

export interface Selection {
  run?: string;
  view: PanelView;
  card?: string;
}

type Signal = { type: "select"; selection: Selection } | { type: "popout-closed" } | { type: "dock" };

function load(): { open: boolean; width: number } {
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? "{}");
    return { open: saved.open !== false, width: typeof saved.width === "number" ? saved.width : 560 };
  } catch {
    return { open: true, width: 560 };
  }
}

function save(state: { open: boolean; width: number }) {
  try {
    localStorage.setItem(KEY, JSON.stringify(state));
  } catch {
    // Private windows may refuse storage; the layout just won't be remembered.
  }
}

/**
 * The right panel's layout in the main window: open or closed, its width,
 * and whether it lives in its own pop-out window. Width and open state are
 * remembered per browser profile; the selection syncs with the pop-out.
 */
export function usePanelLayout(onRemoteSelect: (s: Selection) => void) {
  const [{ open, width }, setLayout] = useState(load);
  const [popped, setPopped] = useState(false);
  const channel = useRef<BroadcastChannel | null>(null);
  const container = useRef<HTMLDivElement>(null);
  const remote = useRef(onRemoteSelect);
  remote.current = onRemoteSelect;

  useEffect(() => save({ open, width }), [open, width]);

  useEffect(() => {
    const ch = new BroadcastChannel(CHANNEL);
    ch.onmessage = (event: MessageEvent<Signal>) => {
      if (event.data.type === "select") remote.current(event.data.selection);
      if (event.data.type === "popout-closed") setPopped(false);
    };
    channel.current = ch;
    return () => ch.close();
  }, []);

  const clamp = useCallback((next: number) => {
    const total = container.current?.clientWidth ?? 1200;
    return Math.max(MIN, Math.min(next, total - MIN_CHAT));
  }, []);

  const startResize = (event: PointerEvent<HTMLDivElement>) => {
    const box = container.current?.getBoundingClientRect();
    if (!box) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    const move = (e: globalThis.PointerEvent) => setLayout((s) => ({ ...s, width: clamp(box.right - e.clientX) }));
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  const resizeByKey = (event: KeyboardEvent<HTMLDivElement>) => {
    const step = event.shiftKey ? 96 : 24;
    if (event.key === "ArrowLeft") setLayout((s) => ({ ...s, width: clamp(s.width + step) }));
    else if (event.key === "ArrowRight") setLayout((s) => ({ ...s, width: clamp(s.width - step) }));
    else return;
    event.preventDefault();
  };

  const popout = (selection: Selection) => {
    const url = new URL(window.location.href);
    url.search = "";
    url.searchParams.set("popout", "panel");
    url.searchParams.set("view", selection.view);
    if (selection.run) url.searchParams.set("run", selection.run);
    if (selection.card) url.searchParams.set("card", selection.card);
    const win = window.open(url, "project-o-panel", "popup,width=760,height=880");
    if (win) setPopped(true);
  };

  const dock = () => {
    channel.current?.postMessage({ type: "dock" } satisfies Signal);
    setPopped(false);
    setLayout((s) => ({ ...s, open: true }));
  };

  return {
    open,
    width,
    popped,
    container,
    setOpen: (next: boolean) => setLayout((s) => ({ ...s, open: next })),
    startResize,
    resizeByKey,
    popout,
    dock,
    broadcast: (selection: Selection) => channel.current?.postMessage({ type: "select", selection } satisfies Signal),
  };
}

/** The pop-out window's side of the channel: follow the main window, report changes, say goodbye. */
export function usePopoutChannel(onRemoteSelect: (s: Selection) => void) {
  const channel = useRef<BroadcastChannel | null>(null);
  const remote = useRef(onRemoteSelect);
  remote.current = onRemoteSelect;
  useEffect(() => {
    const ch = new BroadcastChannel(CHANNEL);
    ch.onmessage = (event: MessageEvent<Signal>) => {
      if (event.data.type === "select") remote.current(event.data.selection);
      if (event.data.type === "dock") window.close();
    };
    const bye = () => ch.postMessage({ type: "popout-closed" } satisfies Signal);
    window.addEventListener("pagehide", bye);
    channel.current = ch;
    return () => {
      window.removeEventListener("pagehide", bye);
      ch.close();
    };
  }, []);
  return (selection: Selection) => channel.current?.postMessage({ type: "select", selection } satisfies Signal);
}
