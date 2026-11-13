/**
 * The 3D canvas of the Graph and Dreaming tabs (ADR-0033, ADR-0041): a thin React wrapper
 * around `GraphScene`, which draws the whole graph in a few batched draw calls from
 * positions a worker computed. The 3D libraries load lazily so the main bundle stays
 * small. On a scope's first load the graph grows level by level — each node out of its
 * parent — then glow and flow light up; with effects off or reduced motion everything
 * appears at once. Colors come from the app's CSS variables and the 색 기준 setting.
 */
import { useEffect, useRef, useState } from "react";

import type { GraphColorBy, GraphNode } from "../../platform/backend/types.ts";
import { loadGraphLibs } from "./graphLib.ts";
import { GraphScene, type SceneView } from "./graphScene.ts";

/** A layout this quick never shows its "배치하는 중" chip. */
const BUSY_CHIP_AFTER_MS = 250;

export function Graph3D({ view, loading, effects, particles, reducedMotion, colorBy, dark, onNodeClick, className, label = "코드 그래프" }: {
  view: SceneView;
  /** First-scope data still arriving (overall parts); the reveal keeps growing while it is true. */
  loading: boolean;
  effects: boolean;
  particles: boolean;
  reducedMotion: boolean;
  /** The 색 기준 setting: by file language or by top-level area. */
  colorBy: GraphColorBy;
  /** The app theme — picks the dark or light color tables. */
  dark: boolean;
  onNodeClick?: (node: GraphNode) => void;
  className?: string;
  label?: string;
}) {
  const host = useRef<HTMLDivElement>(null);
  const scene = useRef<GraphScene | null>(null);
  /** "Fly then open" timeouts; a scope change or unmount cancels them. */
  const timers = useRef(new Set<number>());
  /** The props the scene's callbacks read, so they never go stale. */
  const live = useRef({ view, loading, effects, particles, reducedMotion, colorBy, dark, onNodeClick });
  const [tip, setTip] = useState<{ text: string; x: number; y: number }>();
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    live.current = { view, loading, effects, particles, reducedMotion, colorBy, dark, onNodeClick };
  });

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    let disposed = false;
    let busyTimer = 0;
    const pending = timers.current;

    void loadGraphLibs()
      .then((libs) => {
        if (disposed) return;
        const made = new GraphScene(el, libs, {
          /** Overall-scope project clicks fly the camera in, then hand the node up. */
          onClick: (node) => {
            const { view: v, reducedMotion: rm, onNodeClick: cb } = live.current;
            if (v.scope === "overall" && node.kind === "project" && !rm && made.flyTo(node.id, 900)) {
              const t = window.setTimeout(() => {
                pending.delete(t);
                live.current.onNodeClick?.(node);
              }, 950);
              pending.add(t);
              return;
            }
            cb?.(node);
          },
          onHover: (node, x, y) => setTip(node ? { text: node.label, x, y } : undefined),
          onBusy: (on) => {
            window.clearTimeout(busyTimer);
            if (on) busyTimer = window.setTimeout(() => setBusy(true), BUSY_CHIP_AFTER_MS);
            else setBusy(false);
          },
        });
        scene.current = made;
        const { onNodeClick: _, view: v, ...options } = live.current;
        made.update(v, options);
      })
      .catch((error) => {
        console.error("Graph3D: the 3D libraries failed to load", error);
      });

    return () => {
      disposed = true;
      window.clearTimeout(busyTimer);
      for (const t of pending) window.clearTimeout(t);
      pending.clear();
      scene.current?.dispose();
      scene.current = null;
    };
  }, []);

  const scopeKey = view.scope === "overall" ? "overall" : (view.project ?? view.scope);
  useEffect(() => {
    // A pending "fly then open" belongs to the scope it started on.
    for (const t of timers.current) window.clearTimeout(t);
    timers.current.clear();
    setTip(undefined);
  }, [scopeKey]);

  useEffect(() => {
    scene.current?.update(view, { loading, effects, particles, reducedMotion, colorBy, dark });
  }, [view, loading, effects, particles, reducedMotion, colorBy, dark]);

  useEffect(() => {
    // The scene read its colors when it was made; a frame later the new theme's are in the page.
    const frame = requestAnimationFrame(() => scene.current?.retheme());
    return () => cancelAnimationFrame(frame);
  }, [dark]);

  return (
    <div className={className}>
      <div ref={host} role="img" aria-label={label} className="absolute inset-0" />
      {tip && (
        <p aria-hidden="true" className="float-tooltip-kap" style={{ left: tip.x + 12, top: tip.y + 12 }}>
          {tip.text}
        </p>
      )}
      {busy && (
        <p role="status" className="absolute inset-x-0 bottom-2 z-10 mx-auto w-fit rounded-md border bg-card px-3 py-1.5 text-xs text-muted-foreground shadow">
          그래프를 배치하는 중…
        </p>
      )}
    </div>
  );
}
