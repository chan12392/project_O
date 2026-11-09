import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";

import { readStored, writeStored } from "../lib/storage.ts";
import type { PanelView } from "../data/useRun.ts";
import { popoutUrl } from "./popoutUrl.ts";

const KEY = "project-o.panel";
const CHANNEL = "project-o-panel";
const POPOUT_CLOSED = "project-o:popout-closed";
const MIN = 320;
const MIN_CHAT = 360;

export interface Selection {
  project?: string;
  run?: string;
  view: PanelView;
  card?: string;
  /** The project's open lead tab (ADR-0034); the pop-out panel follows it. */
  thread?: string;
}

type Signal = { type: "select"; selection: Selection } | { type: "popout-closed" } | { type: "dock" };

type Layout = { open: boolean; width: number };

const load = () =>
  readStored<Layout>(KEY, { open: true, width: 560 }, (saved) => {
    const s = saved as Partial<Layout> | null;
    return s && typeof s === "object" ? { open: s.open !== false, width: typeof s.width === "number" ? s.width : 560 } : undefined;
  });

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
  // A drag saves once when it ends, not on every move.
  const dragging = useRef(false);

  useEffect(() => {
    if (!dragging.current) writeStored(KEY, { open, width });
  }, [open, width]);

  useEffect(() => {
    const ch = new BroadcastChannel(CHANNEL);
    ch.onmessage = (event: MessageEvent<Signal>) => {
      if (event.data.type === "select") remote.current(event.data.selection);
      if (event.data.type === "popout-closed") setPopped(false);
    };
    channel.current = ch;
    // The desktop shell says when the pop-out window closed (app/src-tauri/src/commands/window.rs).
    const closed = () => setPopped(false);
    window.addEventListener(POPOUT_CLOSED, closed);
    return () => {
      ch.close();
      window.removeEventListener(POPOUT_CLOSED, closed);
    };
  }, []);

  const clamp = useCallback((next: number) => {
    const total = container.current?.clientWidth ?? 1200;
    return Math.max(MIN, Math.min(next, total - MIN_CHAT));
  }, []);

  const startResize = (event: PointerEvent<HTMLDivElement>) => {
    const box = container.current?.getBoundingClientRect();
    if (!box) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    dragging.current = true;
    let next = width;
    const move = (e: globalThis.PointerEvent) => {
      next = clamp(box.right - e.clientX);
      setLayout((s) => ({ ...s, width: next }));
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
      dragging.current = false;
      writeStored(KEY, { open, width: next });
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
  };

  const resizeByKey = (event: KeyboardEvent<HTMLDivElement>) => {
    const step = event.shiftKey ? 96 : 24;
    if (event.key === "ArrowLeft") setLayout((s) => ({ ...s, width: clamp(s.width + step) }));
    else if (event.key === "ArrowRight") setLayout((s) => ({ ...s, width: clamp(s.width - step) }));
    else return;
    event.preventDefault();
  };

  /**
   * `source`: the runs the main window shows, so the pop-out shows the same ones. The desktop
   * app refuses `window.open`, so its shell opens the pop-out window itself, always on live runs.
   */
  const popout = (selection: Selection, source?: "live" | "demo") => {
    const tauri = window.__TAURI_INTERNALS__;
    if (tauri) {
      const { view, project, run, card } = selection;
      void tauri.invoke("popout_open", { view, project: project ?? null, run: run ?? null, card: card ?? null, thread: selection.thread ?? null }).then(
        () => setPopped(true),
        () => setPopped(false),
      );
      return;
    }
    const win = window.open(popoutUrl(window.location.href, selection, source), "project-o-panel", "popup,width=760,height=880");
    if (win) setPopped(true);
  };

  const dock = () => {
    channel.current?.postMessage({ type: "dock" } satisfies Signal);
    void window.__TAURI_INTERNALS__?.invoke("popout_close").catch(() => {});
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
