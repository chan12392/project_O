/**
 * The Graph tab's canvas (ADR-0033): a thin wrapper around 3d-force-graph that loads
 * the library and three lazily so the main bundle stays small. On a scope's first load
 * the graph grows level by level — each node spawning at its parent's position — then
 * glow and particles light up, so it reads as "finding the connection". With effects
 * off or reduced motion everything appears at once. Node colors come from the app's
 * CSS variables and the 색 기준 setting, so both themes work.
 */
import { useEffect, useRef } from "react";

import type { ForceGraph3DInstance, LinkObject, NodeObject } from "3d-force-graph";

import type { GraphColorBy, GraphLink, GraphNode, GraphView } from "../../platform/backend/types.ts";
import { baseColor, type GraphTheme } from "./graphColors.ts";
import { loadGraphLibs } from "./graphLib.ts";
import { levelsOf, parentsOf } from "./graphLoad.ts";
import { escapeHtml } from "./graphText.ts";
import type { GraphPalette } from "./graphStyle.ts";
import {
  hasGlow,
  linkColor,
  linkParticles,
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

/** How fast the reveal queue drains; the batch size adapts so the whole reveal stays near REVEAL_MS. */
// Each tick hands the graph new data (a layout reheat and a scene update): fewer, larger batches stay smooth.
const REVEAL_TICK_MS = 100;
const REVEAL_MS = 1_800;

/** A scope's identity for the reveal: project→project is a scope change even though `scope` is both "project". */
const scopeKeyOf = (view: GraphView) => (view.scope === "overall" ? "overall" : (view.project ?? view.scope));

const endOf = (end: string | GNode): string => (typeof end === "string" ? end : end.id);

export function Graph3D({ view, loading, effects, particles, reducedMotion, colorBy, dark, onNodeClick, className }: {
  view: GraphView;
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
}) {
  const host = useRef<HTMLDivElement>(null);
  const graphRef = useRef<GGraph | null>(null);
  /** "Fly then open" timeouts; a scope change or unmount cancels them. */
  const timers = useRef(new Set<number>());
  /** The props mount-time accessors and library callbacks read, so they never go stale. */
  const live = useRef({ view, loading, effects, particles, reducedMotion, colorBy, dark, onNodeClick });
  /** The scope on screen; its change cancels a pending fly and starts a fresh reveal. */
  const scope = useRef(scopeKeyOf(view));
  /** Nodes already handed to the graph (their x/y/z live on the objects). */
  const placed = useRef(new Map<string, GNode>());
  /** Reveal queue: nodes waiting to spawn, flushed lowest-level first. */
  const queue = useRef<GNode[]>([]);
  /** Depth in the contains tree per node, recomputed on every sync. */
  const levels = useRef(new Map<string, number>());
  /** Each node's contains parent, recomputed on every sync (spawning looked it up per node over every link). */
  const parents = useRef(new Map<string, string>());
  /**
   * The link objects handed to the graph, kept per link: the library joins links by object, so a
   * fresh copy each flush tore down and rebuilt every line in the scene on every reveal tick.
   */
  const linkObjects = useRef(new Map<string, GLink>());
  /** The drain interval and the moment the current batch must be fully placed by. */
  const draining = useRef(0);
  const deadline = useRef(0);
  /** The reveal finished for this scope visit; same-scope refreshes never re-run it. */
  const revealDone = useRef(false);
  /** Glow/particles suppressed while the reveal runs; flipped on at the end. */
  const revealing = useRef(false);

  useEffect(() => {
    live.current = { view, loading, effects, particles, reducedMotion, colorBy, dark, onNodeClick };
  });

  /** The reveal only runs while effects are on, motion is allowed and this scope visit hasn't finished one. */
  const wantReveal = () => live.current.effects && !live.current.reducedMotion && !revealDone.current;

  /** Where a node spawns: its contains parent's position, so it grows outward from it. */
  const spawnAt = (n: GNode) => {
    const parentId = parents.current.get(n.id);
    const parent = parentId === undefined ? undefined : placed.current.get(parentId);
    const at = parent && [parent.x, parent.y, parent.z].every((c) => typeof c === "number" && Number.isFinite(c)) ? parent : undefined;
    const jitter = () => (Math.random() - 0.5) * 8;
    n.x = (at?.x ?? 0) + jitter();
    n.y = (at?.y ?? 0) + jitter();
    n.z = (at?.z ?? 0) + jitter();
  };

  /** Writes the placed nodes and every link whose ends both exist to the graph — camera and layout intact. */
  const flush = () => {
    const graph = graphRef.current;
    if (!graph) return;
    const ids = new Set(placed.current.keys());
    const links: GLink[] = [];
    const kept = new Set<string>();
    for (const l of live.current.view.links) {
      const source = endOf(l.source);
      const target = endOf(l.target);
      if (!ids.has(source) || !ids.has(target)) continue;
      const key = `${source}|${target}|${l.kind}`;
      if (kept.has(key)) continue;
      kept.add(key);
      let object = linkObjects.current.get(key);
      if (!object) {
        object = { ...l, source, target };
        linkObjects.current.set(key, object);
      } else {
        // Same object, fresh glow/weight: the library keeps its line and only repaints it.
        object.glow = l.glow;
        object.weight = l.weight;
      }
      links.push(object);
    }
    for (const key of [...linkObjects.current.keys()]) if (!kept.has(key)) linkObjects.current.delete(key);
    graph.graphData({ nodes: [...placed.current.values()], links });
  };

  const stopDrain = () => {
    if (draining.current) window.clearInterval(draining.current);
    draining.current = 0;
  };

  const place = (nodes: GNode[]) => {
    for (const n of nodes) {
      spawnAt(n);
      placed.current.set(n.id, n);
    }
  };

  /** The reveal ends once the queue is empty and the first load finished: glow and particles come on. */
  const finishReveal = () => {
    // A lone root (the project list not here yet) has not been revealed: what arrives next still grows.
    if (revealDone.current || queue.current.length > 0 || live.current.loading || placed.current.size <= 1) return;
    revealDone.current = true;
    revealing.current = false;
    graphRef.current?.refresh();
  };

  /** The camera frames the whole graph once per scope visit, when the layout settles; a refresh keeps the person's view. */
  const fitPending = useRef(true);
  const fitOnce = () => {
    const graph = graphRef.current;
    if (!fitPending.current || !graph || placed.current.size <= 1 || live.current.loading) return;
    fitPending.current = false;
    graph.zoomToFit(live.current.reducedMotion ? 0 : 700, 40);
  };

  const startDrain = () => {
    if (draining.current) return;
    draining.current = window.setInterval(() => {
      const left = Math.max(1, Math.ceil((deadline.current - performance.now()) / REVEAL_TICK_MS));
      const take = Math.max(1, Math.ceil(queue.current.length / left));
      queue.current.sort((a, b) => (levels.current.get(a.id) ?? 99) - (levels.current.get(b.id) ?? 99));
      place(queue.current.splice(0, take));
      flush();
      if (queue.current.length === 0) stopDrain();
      finishReveal();
    }, REVEAL_TICK_MS);
  };

  /**
   * The view changed: kept nodes update in place (positions survive), gone nodes leave,
   * new ones either join the reveal queue or land at once. Incremental — never a remount.
   */
  const sync = (v: GraphView) => {
    if (!graphRef.current) return;
    const ids = new Set(v.nodes.map((n) => n.id));
    for (const id of [...placed.current.keys()]) if (!ids.has(id)) placed.current.delete(id);
    queue.current = queue.current.filter((n) => ids.has(n.id));
    const byId = new Map(v.nodes.map((n) => [n.id, n]));
    const RESET: Partial<GraphNode> = { project: undefined, path: undefined, glow: undefined, summary: undefined, indexed: undefined, error: undefined };
    for (const [id, gn] of placed.current) {
      const fresh = byId.get(id);
      if (fresh) Object.assign(gn, RESET, fresh);
    }
    parents.current = parentsOf(v);
    levels.current = levelsOf(v, parents.current);
    const queued = new Set(queue.current.map((n) => n.id));
    const fresh = v.nodes.filter((n) => !placed.current.has(n.id) && !queued.has(n.id)).map((n): GNode => ({ ...n }));
    if (wantReveal() && fresh.length > 0) {
      fresh.sort((a, b) => (levels.current.get(a.id) ?? 99) - (levels.current.get(b.id) ?? 99));
      const now = performance.now();
      if (deadline.current < now) deadline.current = now + REVEAL_MS;
      queue.current.push(...fresh);
      startDrain();
      return;
    }
    if (queue.current.length > 0) {
      // The reveal was switched off mid-way (effects off / reduced motion): land everything at once.
      place(queue.current.splice(0));
      stopDrain();
    }
    place(fresh);
    flush();
    finishReveal();
  };

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
    const pixel = document.createElement("canvas");
    pixel.width = pixel.height = 1;
    const paint = pixel.getContext("2d", { willReadFrequently: true });
    const channel = (v: number) => Math.round(Math.min(255, Math.max(0, v))).toString(16).padStart(2, "0");
    /**
     * Any CSS color (hex, rgb, oklch — Tailwind 4's palette —, color-mix) as [r, g, b, a] in sRGB,
     * by painting one pixel and reading it back. Parsing the canvas's color string failed for
     * oklch, which it does not serialize as hex or rgb(), and those nodes came out black.
     */
    const channels = (raw: string): [number, number, number, number] | null => {
      if (!paint || !raw.trim()) return null;
      paint.clearRect(0, 0, 1, 1);
      paint.fillStyle = "#000";
      paint.fillStyle = raw;
      paint.fillRect(0, 0, 1, 1);
      const [r, g, b, a] = paint.getImageData(0, 0, 1, 1).data;
      return [r, g, b, a / 255];
    };
    const bg: [number, number, number, number] = channels(style.getPropertyValue("--background")) ?? [10, 10, 10, 1];
    /** A CSS color WebGL can hold: composited over the app background, since three ignores alpha. */
    /**
     * Cached: the readback is a synchronous GPU round trip, and the library asks for every
     * link's color on every repaint — a few distinct colors, asked thousands of times.
     */
    const converted = new Map<string, string>();
    const cssColor = (raw: string): string => {
      const known = converted.get(raw);
      if (known !== undefined) return known;
      const c = channels(raw);
      const a = c ? c[3] : 1;
      const hex = c ? `#${channel(c[0] * a + bg[0] * (1 - a))}${channel(c[1] * a + bg[1] * (1 - a))}${channel(c[2] * a + bg[2] * (1 - a))}` : "#000000";
      converted.set(raw, hex);
      return hex;
    };
    const pal: GraphPalette = palette((name) => {
      const raw = style.getPropertyValue(name).trim();
      return raw ? cssColor(raw) : raw;
    });
    /** Glow and particles are read through this so the reveal can hold them back until it ends. */
    const glowOn = () => live.current.effects && !revealing.current;

    void loadGraphLibs()
      .then(({ ForceGraph3D, three }) => {
        if (disposed) return;
        const T = three as Three;
        // Low-poly on purpose: hundreds of nodes share this geometry, and at graph scale 8×6 reads as round.
        sphere = new T.SphereGeometry(1, 8, 6);
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
          const { view: v, colorBy: by, dark: isDark } = live.current;
          const glow = glowOn();
          const dimmed = glow && hasGlow(v) && !n.glow;
          const theme: GraphTheme = isDark ? "dark" : "light";
          // Never the dimmed form: three cannot read color-mix() and paints it white. Dimming is the opacity below.
          const color = nodeColor(n, pal, glow, false, baseColor(n, by, theme, pal));
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
          const size = nodeSize(n, glow);
          const group = new T.Group();
          const mesh = new T.Mesh(geo, material);
          mesh.scale.setScalar(size);
          group.add(mesh);
          const halo = nodeHalo(n, glow);
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
          // The library's English mouse hint; the canvas speaks for itself.
          .showNavInfo(false)
          .nodeId("id")
          // The tooltip writes this string with innerHTML; labels can carry Deep's LLM output.
          .nodeLabel((n) => escapeHtml(n.label))
          .nodeThreeObject(nodeObject)
          // Width 0 draws each link as a plain straight line instead of a cylinder mesh;
          // a glowing link stands out by its color and the flowing dots.
          .linkWidth(0)
          .linkColor((l) => cssColor(linkColor(l, pal, glowOn(), glowOn() && hasGlow(live.current.view))))
          .linkOpacity(0.6)
          .linkDirectionalParticles((l) => linkParticles(l, glowOn(), live.current.particles && !live.current.reducedMotion))
          .linkDirectionalParticleColor(() => pal.linkGlow)
          .linkDirectionalParticleWidth(1.6)
          .linkDirectionalParticleSpeed(() => 0.008)
          .onNodeClick(clicked)
          .onEngineStop(fitOnce)
          .width(el.clientWidth)
          .height(el.clientHeight);

        if (live.current.effects && !live.current.reducedMotion) {
          // The reveal lets the simulation run as nodes grow (bounded); without it the
          // layout settles before the first frame.
          revealing.current = true;
          graph.cooldownTime(4_000);
        } else {
          graph.warmupTicks(80);
        }
        sync(live.current.view);

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
      stopDrain();
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const key = scopeKeyOf(view);
    if (scope.current !== key) {
      scope.current = key;
      for (const t of timers.current) window.clearTimeout(t);
      timers.current.clear();
      // A new scope reveals again; everything from the old scope leaves.
      placed.current.clear();
      linkObjects.current.clear();
      queue.current = [];
      revealDone.current = false;
      fitPending.current = true;
      stopDrain();
      revealing.current = live.current.effects && !live.current.reducedMotion;
    }
    sync(view);
  }, [view]);

  useEffect(() => {
    if (!revealDone.current && !wantReveal()) {
      // The reveal was switched off mid-way: land everything at once, then paint as usual.
      place(queue.current.splice(0));
      stopDrain();
      flush();
    }
    finishReveal();
    graphRef.current?.refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [effects, particles, reducedMotion, colorBy, dark, loading]);

  return <div ref={host} role="img" aria-label="코드 그래프" className={className} />;
}
