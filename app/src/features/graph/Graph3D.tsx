/**
 * The Graph tab's canvas (ADR-0033): a thin wrapper around 3d-force-graph that loads
 * the library and three lazily so the main bundle stays small. The div stays empty
 * while they load — the parent shows its own loading state; a failed import logs and
 * leaves it empty. Colors come from the app's CSS variables, so both themes work.
 */
import { useEffect, useRef } from "react";

import type { ForceGraph3DInstance, LinkObject, NodeObject } from "3d-force-graph";

import type { GraphLink, GraphNode, GraphView } from "../../platform/backend/types.ts";
import { escapeHtml } from "./graphText.ts";
import type { GraphPalette } from "./graphStyle.ts";
import {
  hasGlow,
  linkColor,
  linkParticles,
  linkWidth,
  nodeColor,
  nodeHalo,
  nodeSize,
  palette,
} from "./graphStyle.ts";

/** Nodes and links as the simulation sees them; it writes x/y/z back onto them. */
type GNode = GraphNode & NodeObject;
type GLink = GraphLink & LinkObject<GNode>;
type GGraph = ForceGraph3DInstance<GNode, GLink>;

/** The slice of three the node objects need; the package ships no type declarations. */
interface Three {
  Group: new () => ThreeObj;
  Mesh: new (geometry: ThreeGeometry, material: ThreeMaterial) => ThreeObj;
  SphereGeometry: new (radius: number, widthSegments: number, heightSegments: number) => ThreeGeometry;
  MeshBasicMaterial: new (params?: { color?: string }) => ThreeMaterial;
  Sprite: new (material: ThreeMaterial) => ThreeObj;
  SpriteMaterial: new (params?: {
    map?: ThreeTexture;
    color?: string;
    transparent?: boolean;
    depthWrite?: boolean;
  }) => ThreeMaterial;
  CanvasTexture: new (canvas: HTMLCanvasElement) => ThreeTexture;
}
interface ThreeObj {
  add(...children: ThreeObj[]): void;
  scale: { set(x: number, y: number, z: number): void; setScalar(size: number): void };
}
interface ThreeGeometry {
  dispose(): void;
}
interface ThreeTexture {
  dispose(): void;
}
interface ThreeMaterial {
  transparent: boolean;
  opacity: number;
  dispose(): void;
}

/** Copies the library can write x/y/z and index on without mutating the caller's view. */
const copies = (view: GraphView): { nodes: GNode[]; links: GLink[] } => ({
  nodes: view.nodes.map((n) => ({ ...n })),
  links: view.links.map((l) => ({ ...l })),
});

export function Graph3D({ view, effects, particles, reducedMotion, onNodeClick, className }: {
  view: GraphView;
  effects: boolean;
  particles: boolean;
  reducedMotion: boolean;
  onNodeClick?: (node: GraphNode) => void;
  className?: string;
}) {
  const host = useRef<HTMLDivElement>(null);
  const graphRef = useRef<GGraph | null>(null);
  /** "Fly then open" timeouts; a scope change or unmount cancels them. */
  const timers = useRef(new Set<number>());
  /** The props mount-time accessors and library callbacks read, so they never go stale. */
  const live = useRef({ view, effects, particles, reducedMotion, onNodeClick });
  /** The scope on screen; its change cancels a pending fly. */
  const scope = useRef(view.scope);

  useEffect(() => {
    live.current = { view, effects, particles, reducedMotion, onNodeClick };
  });

  useEffect(() => {
    const el = host.current;
    if (!el) return;

    let disposed = false;
    let graph: GGraph | null = null;
    let observer: ResizeObserver | null = null;
    let sphere: ThreeGeometry | null = null;
    let haloTexture: ThreeTexture | null = null;
    /** Node materials keyed by "color" or "color|dim"; sprite materials keyed by tint. */
    const materials = new Map<string, ThreeMaterial>();
    const sprites = new Map<string, ThreeMaterial>();

    const style = getComputedStyle(el);
    const paint = document.createElement("canvas").getContext("2d");
    const channel = (v: number) => Math.round(Math.min(255, Math.max(0, v))).toString(16).padStart(2, "0");
    /** "#rrggbb" or "rgb(a)(r, g, b[, a])" as canvas serializes them → [r, g, b, a]. */
    const channels = (s: string): [number, number, number, number] | null => {
      const h = /^#([0-9a-f]{6})$/i.exec(s.trim());
      if (h) {
        const v = parseInt(h[1], 16);
        return [(v >> 16) & 255, (v >> 8) & 255, v & 255, 1];
      }
      const m = /^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*(?:,\s*([\d.]+)\s*)?\)$/i.exec(s.trim());
      if (m) {
        const a = m[4] === undefined ? 1 : Number(m[4]);
        return [Number(m[1]), Number(m[2]), Number(m[3]), Number.isFinite(a) ? a : 1];
      }
      return null;
    };
    /** Canvas-serializes any CSS color (oklch, slash-rgb, color-mix) to hex or rgba(). */
    const serialize = (raw: string): string => {
      if (!paint) return raw;
      paint.fillStyle = "#000";
      paint.fillStyle = raw;
      return paint.fillStyle;
    };
    const bg: [number, number, number, number] = channels(serialize(style.getPropertyValue("--background"))) ?? [10, 10, 10, 1];
    /** A CSS color WebGL can hold: composited over the app background, since three ignores alpha. */
    const cssColor = (raw: string): string => {
      const c = channels(serialize(raw));
      if (!c) return "#000000";
      const a = c[3];
      return `#${channel(c[0] * a + bg[0] * (1 - a))}${channel(c[1] * a + bg[1] * (1 - a))}${channel(c[2] * a + bg[2] * (1 - a))}`;
    };
    const pal: GraphPalette = palette((name) => {
      const raw = style.getPropertyValue(name).trim();
      return raw ? cssColor(raw) : raw;
    });

    void Promise.all([
      import("3d-force-graph"),
      // @ts-expect-error three ships no type declarations; `Three` above is the slice nodes use.
      import("three"),
    ])
      .then(([mod, three]) => {
        if (disposed) return;
        const T = three as Three;
        const { default: ForceGraph3D } = mod;
        sphere = new T.SphereGeometry(1, 12, 12);
        const geo = sphere;

        /** The shared white→transparent sprite image behind glowing nodes. */
        const glowTexture = (): ThreeTexture => {
          if (!haloTexture) {
            const canvas = document.createElement("canvas");
            canvas.width = canvas.height = 128;
            const ctx = canvas.getContext("2d");
            if (ctx) {
              const g = ctx.createRadialGradient(64, 64, 0, 64, 64, 64);
              g.addColorStop(0, "rgba(255,255,255,0.9)");
              g.addColorStop(0.35, "rgba(255,255,255,0.35)");
              g.addColorStop(1, "rgba(255,255,255,0)");
              ctx.fillStyle = g;
              ctx.fillRect(0, 0, 128, 128);
            }
            haloTexture = new T.CanvasTexture(canvas);
          }
          return haloTexture;
        };
        const spriteMaterial = (n: GNode): ThreeMaterial => {
          const tint = n.glow === "active" ? pal.active : pal.path;
          let material = sprites.get(tint);
          if (!material) {
            material = new T.SpriteMaterial({ map: glowTexture(), color: tint, transparent: true, depthWrite: false });
            sprites.set(tint, material);
          }
          return material;
        };
        /** A scaled sphere, plus a tinted halo sprite when the node glows. */
        const nodeObject = (n: GNode): ThreeObj => {
          const { effects: eff, view: v } = live.current;
          const dimmed = eff && hasGlow(v) && !n.glow;
          const color = nodeColor(n, pal, eff, false);
          const key = dimmed ? `${color}|dim` : color;
          let material = materials.get(key);
          if (!material) {
            material = new T.MeshBasicMaterial({ color });
            // color-mix strings are not valid WebGL colors, so dimming uses opacity instead.
            if (dimmed) {
              material.transparent = true;
              material.opacity = pal.dim;
            }
            materials.set(key, material);
          }
          const size = nodeSize(n, eff);
          const group = new T.Group();
          const mesh = new T.Mesh(geo, material);
          mesh.scale.setScalar(size);
          group.add(mesh);
          const halo = nodeHalo(n, eff);
          if (halo > 0) {
            const sprite = new T.Sprite(spriteMaterial(n));
            const s = size * (1 + halo);
            sprite.scale.set(s, s, 1);
            group.add(sprite);
          }
          return group;
        };
        /** Overall-scope project clicks fly the camera in, then hand the node up. */
        const clicked = (n: GNode) => {
          const { view: v, reducedMotion: rm, onNodeClick: cb } = live.current;
          if (v.scope === "overall" && n.kind === "project" && !rm) {
            const { x, y, z } = n;
            if ([x, y, z].every((c) => typeof c === "number" && Number.isFinite(c))) {
              graph!.cameraPosition({ x: x! * 1.25, y: y! * 1.25, z: z! * 1.25 }, { x: x!, y: y!, z: z! }, 900);
            }
            const t = window.setTimeout(() => {
              timers.current.delete(t);
              live.current.onNodeClick?.(n);
            }, 950);
            timers.current.add(t);
            return;
          }
          cb?.(n);
        };

        graph = new ForceGraph3D(el) as unknown as GGraph;
        graphRef.current = graph;
        graph
          .backgroundColor("rgba(0,0,0,0)")
          .nodeId("id")
          // The tooltip writes this string with innerHTML; labels can carry Deep's LLM output.
          .nodeLabel((n) => escapeHtml(n.label))
          .nodeResolution(12)
          .nodeThreeObject(nodeObject)
          .linkWidth((l) => linkWidth(l, live.current.effects))
          .linkColor((l) => cssColor(linkColor(l, pal, live.current.effects, hasGlow(live.current.view))))
          .linkOpacity(0.6)
          .linkDirectionalParticles((l) => linkParticles(l, live.current.effects, live.current.particles && !live.current.reducedMotion))
          .linkDirectionalParticleColor(() => pal.linkGlow)
          .linkDirectionalParticleWidth(1.6)
          .linkDirectionalParticleSpeed(() => 0.008)
          .onNodeClick(clicked)
          .graphData(copies(live.current.view))
          .width(el.clientWidth)
          .height(el.clientHeight);

        observer = new ResizeObserver((entries) => {
          const rect = entries[0]?.contentRect;
          if (rect) graph?.width(rect.width).height(rect.height);
        });
        observer.observe(el);
      })
      .catch((error) => {
        console.error("Graph3D: 3d-force-graph failed to load", error);
      });

    return () => {
      disposed = true;
      observer?.disconnect();
      for (const t of timers.current) window.clearTimeout(t);
      timers.current.clear();
      graph?._destructor();
      if (graphRef.current === graph) graphRef.current = null;
      for (const m of materials.values()) m.dispose();
      for (const m of sprites.values()) m.dispose();
      materials.clear();
      sprites.clear();
      haloTexture?.dispose();
      sphere?.dispose();
    };
  }, []);

  useEffect(() => {
    if (scope.current !== view.scope) {
      scope.current = view.scope;
      for (const t of timers.current) window.clearTimeout(t);
      timers.current.clear();
    }
    graphRef.current?.graphData(copies(view));
  }, [view]);

  useEffect(() => {
    graphRef.current?.refresh();
  }, [effects, particles, reducedMotion]);

  return <div ref={host} role="img" aria-label="코드 그래프" className={className} />;
}
