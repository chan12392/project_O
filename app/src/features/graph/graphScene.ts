/**
 * The graph canvas's own renderer (ADR-0041). 3d-force-graph still owns the WebGL canvas,
 * the camera and the mouse navigation, but it is handed no data: every node is one instance
 * of a single sphere mesh, every link one segment of a single line buffer, every soft light
 * one point of a single point cloud. A graph of thousands of nodes is four draw calls, and
 * nothing is laid out on the main thread — positions come finished from `layoutWorker.ts`.
 * What moves afterwards is only the reveal (nodes growing out of their parents) and the
 * flow along a glowing path.
 */
import type { GraphColorBy, GraphLink, GraphNode } from "../../platform/backend/types.ts";
import { baseColor, type GraphTheme } from "./graphColors.ts";
import type { GraphLibs } from "./graphLib.ts";
import { levelsOf, parentsOf } from "./graphLoad.ts";
import { boundsOf, easeOut, fitDistance, fitSettled, frameWrite, GROW_MS, pickSphere, revealDelay } from "./graphMotion.ts";
import { hasGlow, linkColor, linkParticles, nodeAura, nodeColor, nodeSize, palette, type GraphPalette } from "./graphStyle.ts";
import type { Three, ThreeAttribute, ThreeCamera, ThreeGeometry, ThreeInstanced, ThreeMaterial, ThreeObject, ThreeScene } from "./graphThree.ts";
import type { LayoutAnswer, LayoutRequest } from "./layoutWorker.ts";

/** The graph on screen: a scope's nodes and links (the Graph tab's view or the Dreaming graph). */
export interface SceneView {
  scope: string;
  project?: string;
  nodes: GraphNode[];
  links: GraphLink[];
}

export interface SceneOptions {
  effects: boolean;
  particles: boolean;
  reducedMotion: boolean;
  colorBy: GraphColorBy;
  dark: boolean;
  /** More of this scope is still arriving: the reveal and the camera wait for the rest. */
  loading: boolean;
}

export interface SceneHooks {
  onClick(node: GraphNode): void;
  /** The node under the pointer (canvas-relative position), or none. */
  onHover(node: GraphNode | undefined, x: number, y: number): void;
  /** A layout is being computed. */
  onBusy(busy: boolean): void;
}

/** One node on the canvas: where it is, where it is going, and what it is painted with. */
interface Item {
  node: GraphNode;
  x: number;
  y: number;
  z: number;
  fromX: number;
  fromY: number;
  fromZ: number;
  toX: number;
  toY: number;
  toZ: number;
  /** When its move starts and how long it takes. */
  start: number;
  span: number;
  /** When it starts appearing and how long it grows; `shown` is 0 → 1. */
  born: number;
  grow: number;
  shown: number;
  size: number;
  rgb: Rgb;
  aura: number;
  auraAlpha: number;
  auraRgb: Rgb;
}

interface Segment {
  link: GraphLink;
  source: Item;
  target: Item;
  rgb: Rgb;
  /** Dots flowing along it. */
  dots: number;
}

type Rgb = [number, number, number];

/** The handles of 3d-force-graph this file uses, with three's objects typed by graphThree.ts. */
interface Host {
  backgroundColor(color: string): Host;
  showNavInfo(show: boolean): Host;
  enablePointerInteraction(enable: boolean): Host;
  width(width: number): Host;
  height(height: number): Host;
  scene(): ThreeScene;
  camera(): ThreeCamera;
  renderer(): { domElement: HTMLCanvasElement };
  cameraPosition(): { x: number; y: number; z: number };
  cameraPosition(position: { x: number; y: number; z: number }, lookAt: { x: number; y: number; z: number }, ms: number): Host;
  _destructor(): void;
}

const POINT_VERTEX = `
attribute vec3 aColor;
attribute float aSize;
attribute float aAlpha;
uniform float uScale;
uniform float uTime;
uniform float uPulse;
varying vec3 vColor;
varying float vAlpha;
void main() {
  vec4 mv = modelViewMatrix * vec4(position, 1.0);
  gl_Position = projectionMatrix * mv;
  float breathe = 1.0 + uPulse * sin(uTime * 0.9 + position.x * 0.05 + position.y * 0.07 + position.z * 0.03);
  gl_PointSize = aSize * breathe * uScale / max(0.001, -mv.z);
  vColor = aColor;
  vAlpha = aAlpha;
}`;

const POINT_FRAGMENT = `
varying vec3 vColor;
varying float vAlpha;
void main() {
  float d = length(gl_PointCoord - 0.5) * 2.0;
  if (d > 1.0) discard;
  float falloff = (1.0 - d) * (1.0 - d);
  gl_FragColor = vec4(vColor, falloff * vAlpha);
  #include <colorspace_fragment>
}`;

/** How long a node already on screen takes to move when the layout shifts. */
const MOVE_MS = 600;
/** A node arriving after the first reveal grows this fast. */
const LATE_GROW_MS = 400;
/** Layout requests wait this long so several changes (the overall graph's parts) share one. */
const LAYOUT_WAIT_MS = 60;
const LAYOUT_WAIT_LOADING_MS = 180;
/** Plain links fade toward the background this much; a glowing one keeps its full color. */
const LINK_STRENGTH = 0.6;
/** Loops per second a flowing dot travels along its link. */
const FLOW_SPEED = 0.45;

const scopeKeyOf = (view: SceneView) => (view.scope === "overall" ? "overall" : (view.project ?? view.scope));
const endOf = (end: string | { id: string }): string => (typeof end === "string" ? end : end.id);
/** How far along its move a node is at `time`: 0 → 1. */
const moved = (it: Item, time: number) => (it.span <= 0 ? 1 : easeOut((time - it.start) / it.span));
const mix = (from: Rgb, to: Rgb, t: number): Rgb => [from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t, from[2] + (to[2] - from[2]) * t];

export class GraphScene {
  private readonly three: Three;
  private readonly host: Host;
  private pal!: GraphPalette;
  private background!: Rgb;
  private readonly sphere: ThreeGeometry;
  private readonly nodeMaterial: ThreeMaterial;
  private readonly lineMaterial: ThreeMaterial;
  private readonly auraMaterial: ThreeMaterial;
  private readonly dotMaterial: ThreeMaterial;
  private readonly raycaster: InstanceType<Three["Raycaster"]>;
  private readonly observer: ResizeObserver;
  private worker?: Worker;

  private mesh?: ThreeInstanced;
  private auras?: { object: ThreeObject; geometry: ThreeGeometry; position: ThreeAttribute; color: ThreeAttribute; size: ThreeAttribute; alpha: ThreeAttribute };
  private lines?: { object: ThreeObject; geometry: ThreeGeometry; position: ThreeAttribute; color: ThreeAttribute };
  private dots?: { object: ThreeObject; geometry: ThreeGeometry; position: ThreeAttribute; color: ThreeAttribute; size: ThreeAttribute; alpha: ThreeAttribute; capacity: number };
  private nodeCapacity = 0;
  private linkCapacity = 0;
  /** Per node, how near the pointer must pass to hit it. */
  private radii = new Float32Array(0);

  private view: SceneView = { scope: "", nodes: [], links: [] };
  private options: SceneOptions = { effects: true, particles: true, reducedMotion: false, colorBy: "language", dark: true, loading: false };
  private scope = "";
  private items: Item[] = [];
  private byId = new Map<string, Item>();
  private segments: Segment[] = [];
  private parents = new Map<string, string>();
  private levels = new Map<string, number>();

  /** The node and link set the positions on screen were computed for. */
  private laidOut = "";
  private job = 0;
  private pending?: { request: LayoutRequest; ids: string[]; signature: string; scope: string };
  private layoutTimer = 0;
  /** The reveal finished for this scope visit: glow and flow are on, later arrivals just grow in. */
  private revealDone = false;
  private fitPending = true;
  private animUntil = 0;
  /** A frame inside a move was written: the first frame after the move writes where it ends. */
  private settling = false;
  private dirty = true;
  private raf = 0;
  private disposed = false;

  private pointer?: { x: number; y: number; moved: boolean };
  private press?: { x: number; y: number };
  private hovered?: Item;
  private readonly colors = new Map<string, Rgb>();
  private readonly converted = new Map<string, string>();
  private readonly paint: CanvasRenderingContext2D | null;
  private backgroundChannels: [number, number, number, number] = [10, 10, 10, 1];

  private readonly el: HTMLElement;
  private readonly hooks: SceneHooks;

  constructor(el: HTMLElement, libs: GraphLibs, hooks: SceneHooks) {
    this.el = el;
    this.hooks = hooks;
    const T = (this.three = libs.three as Three);
    const pixel = document.createElement("canvas");
    pixel.width = pixel.height = 1;
    this.paint = pixel.getContext("2d", { willReadFrequently: true });
    this.readTheme();

    this.host = new libs.ForceGraph3D(el) as unknown as Host;
    this.host
      .backgroundColor("rgba(0,0,0,0)")
      // The library's English mouse hint; the canvas speaks for itself.
      .showNavInfo(false)
      // The library would test the pointer against every object each frame; picking is done here, against node spheres only.
      .enablePointerInteraction(false)
      .width(el.clientWidth)
      .height(el.clientHeight);

    // Every node shares this geometry in one instanced draw, so it can afford to be round.
    this.sphere = new T.SphereGeometry(1, 14, 10);
    this.nodeMaterial = new T.MeshBasicMaterial();
    this.lineMaterial = new T.LineBasicMaterial({ vertexColors: true });
    const points = () =>
      new T.ShaderMaterial({
        uniforms: { uScale: { value: 1 }, uTime: { value: 0 }, uPulse: { value: 0 } },
        vertexShader: POINT_VERTEX,
        fragmentShader: POINT_FRAGMENT,
        transparent: true,
        depthWrite: false,
        blending: T.AdditiveBlending,
      });
    this.auraMaterial = points();
    this.dotMaterial = points();
    this.raycaster = new T.Raycaster();
    this.allocate(256, 512);

    try {
      this.worker = new Worker(new URL("./layoutWorker.ts", import.meta.url), { type: "module" });
      this.worker.onmessage = (event: MessageEvent<LayoutAnswer>) => this.onAnswer(event.data);
      this.worker.onerror = () => {
        // No worker (blocked or failed to load): the same layout on this thread, for the request in flight too.
        this.worker?.terminate();
        this.worker = undefined;
        if (this.pending) this.post(this.pending.request);
      };
    } catch {
      this.worker = undefined;
    }

    this.observer = new ResizeObserver((entries) => {
      const rect = entries[0]?.contentRect;
      if (rect) this.host.width(rect.width).height(rect.height);
    });
    this.observer.observe(el);
    el.addEventListener("pointerdown", this.onPointerDown, true);
    el.addEventListener("pointerup", this.onPointerUp, true);
    el.addEventListener("pointermove", this.onPointerMove, true);
    el.addEventListener("pointerleave", this.onPointerLeave, true);
    this.raf = requestAnimationFrame(this.tick);
  }

  /** The palette and the background colors fade toward, as the page's theme has them now. */
  private readTheme(): void {
    const style = getComputedStyle(this.el);
    this.colors.clear();
    this.converted.clear();
    this.backgroundChannels = this.channels(style.getPropertyValue("--background")) ?? [10, 10, 10, 1];
    this.pal = palette((name) => {
      const raw = style.getPropertyValue(name).trim();
      return raw ? this.cssColor(raw) : raw;
    });
    const [r, g, b] = this.backgroundChannels;
    this.background = this.linear(`#${[r, g, b].map((v) => Math.round(v).toString(16).padStart(2, "0")).join("")}`);
  }

  /** The theme changed under a scene that stays: every color is read and painted again. */
  retheme(): void {
    if (this.disposed) return;
    this.readTheme();
    this.restyle();
  }

  /** The view or how it is shown changed: kept nodes stay where they are, the rest is laid out again. */
  update(view: SceneView, options: SceneOptions): void {
    const key = scopeKeyOf(view);
    if (key !== this.scope) {
      // A new scope reveals again; everything from the old scope leaves.
      this.scope = key;
      this.items = [];
      this.byId.clear();
      this.segments = [];
      this.laidOut = "";
      if (this.pending) this.hooks.onBusy(false);
      this.pending = undefined;
      this.revealDone = false;
      this.fitPending = true;
      this.setHovered(undefined);
    }
    this.view = view;
    this.options = options;
    const wanted = new Map(view.nodes.map((n) => [n.id, n]));
    if (this.items.some((it) => !wanted.has(it.node.id))) {
      this.items = this.items.filter((it) => wanted.has(it.node.id));
      this.byId = new Map(this.items.map((it) => [it.node.id, it]));
      if (this.hovered && !wanted.has(this.hovered.node.id)) this.setHovered(undefined);
    }
    for (const it of this.items) it.node = wanted.get(it.node.id) ?? it.node;
    this.parents = parentsOf(view);
    this.levels = levelsOf(view, this.parents);
    if (view.nodes.length === 0) {
      this.laidOut = "";
      // An answer on its way is for nodes that left: placed, it would mark this empty view as laid out.
      if (this.pending) this.hooks.onBusy(false);
      this.pending = undefined;
    } else if (this.signature(view) !== this.laidOut) this.scheduleLayout();
    this.connect();
    this.restyle();
    this.fitWhenReady();
  }

  /** Moves the camera in on a node; false when it is not on screen. */
  flyTo(id: string, ms: number): boolean {
    const it = this.byId.get(id);
    if (!it) return false;
    this.host.cameraPosition({ x: it.toX * 1.25, y: it.toY * 1.25, z: it.toZ * 1.25 }, { x: it.toX, y: it.toY, z: it.toZ }, ms);
    return true;
  }

  dispose(): void {
    this.disposed = true;
    cancelAnimationFrame(this.raf);
    window.clearTimeout(this.layoutTimer);
    this.worker?.terminate();
    this.observer.disconnect();
    this.el.removeEventListener("pointerdown", this.onPointerDown, true);
    this.el.removeEventListener("pointerup", this.onPointerUp, true);
    this.el.removeEventListener("pointermove", this.onPointerMove, true);
    this.el.removeEventListener("pointerleave", this.onPointerLeave, true);
    this.release();
    this.dots?.geometry.dispose();
    this.host._destructor();
    for (const part of [this.sphere, this.nodeMaterial, this.lineMaterial, this.auraMaterial, this.dotMaterial]) part.dispose();
  }

  // Colors

  /** Any CSS color (hex, rgb, oklch, color-mix) as [r, g, b, a] in sRGB, by painting one pixel and reading it back. */
  private channels(raw: string): [number, number, number, number] | null {
    if (!this.paint || !raw.trim()) return null;
    this.paint.clearRect(0, 0, 1, 1);
    this.paint.fillStyle = "#000";
    this.paint.fillStyle = raw;
    this.paint.fillRect(0, 0, 1, 1);
    const [r, g, b, a] = this.paint.getImageData(0, 0, 1, 1).data;
    return [r, g, b, a / 255];
  }

  /** A CSS color as a hex WebGL can hold, composited over the app background. Cached: the readback is a GPU round trip. */
  private cssColor(raw: string): string {
    const known = this.converted.get(raw);
    if (known !== undefined) return known;
    const c = this.channels(raw);
    const bg = this.backgroundChannels;
    const a = c ? c[3] : 1;
    const channel = (v: number) => Math.round(Math.min(255, Math.max(0, v))).toString(16).padStart(2, "0");
    const hex = c ? `#${channel(c[0] * a + bg[0] * (1 - a))}${channel(c[1] * a + bg[1] * (1 - a))}${channel(c[2] * a + bg[2] * (1 - a))}` : "#000000";
    this.converted.set(raw, hex);
    return hex;
  }

  /** A color in three's working space, as the buffers hold it. */
  private linear(color: string): Rgb {
    let rgb = this.colors.get(color);
    if (!rgb) {
      const c = new this.three.Color(color.startsWith("#") ? color : this.cssColor(color));
      rgb = [c.r, c.g, c.b];
      this.colors.set(color, rgb);
    }
    return rgb;
  }

  // Buffers

  private release(): void {
    const scene = this.host.scene();
    if (this.mesh) {
      scene.remove(this.mesh);
      this.mesh.dispose();
    }
    for (const part of [this.auras, this.lines]) {
      if (!part) continue;
      scene.remove(part.object);
      part.geometry.dispose();
    }
    this.mesh = undefined;
    this.auras = undefined;
    this.lines = undefined;
  }

  /** One set of buffers big enough for this many nodes and links; they are rebuilt only when the graph outgrows them. */
  private allocate(nodes: number, links: number): void {
    const T = this.three;
    this.release();
    this.nodeCapacity = nodes;
    this.linkCapacity = links;
    this.radii = new Float32Array(nodes);
    const attribute = (size: number, items: number) => new T.BufferAttribute(new Float32Array(size * items), items);

    const mesh = new T.InstancedMesh(this.sphere, this.nodeMaterial, nodes);
    mesh.instanceColor = new T.InstancedBufferAttribute(new Float32Array(nodes * 3), 3);
    mesh.count = 0;
    // Positions live in the instance buffer, which three's culling does not see.
    mesh.frustumCulled = false;
    this.mesh = mesh;

    const auraGeometry = new T.BufferGeometry();
    const auras = { geometry: auraGeometry, position: attribute(nodes, 3), color: attribute(nodes, 3), size: attribute(nodes, 1), alpha: attribute(nodes, 1) };
    auraGeometry.setAttribute("position", auras.position);
    auraGeometry.setAttribute("aColor", auras.color);
    auraGeometry.setAttribute("aSize", auras.size);
    auraGeometry.setAttribute("aAlpha", auras.alpha);
    auraGeometry.setDrawRange(0, 0);
    const auraObject = new T.Points(auraGeometry, this.auraMaterial);
    auraObject.frustumCulled = false;
    this.auras = { ...auras, object: auraObject };

    const lineGeometry = new T.BufferGeometry();
    const lines = { geometry: lineGeometry, position: attribute(links * 2, 3), color: attribute(links * 2, 3) };
    lineGeometry.setAttribute("position", lines.position);
    lineGeometry.setAttribute("color", lines.color);
    lineGeometry.setDrawRange(0, 0);
    const lineObject = new T.LineSegments(lineGeometry, this.lineMaterial);
    lineObject.frustumCulled = false;
    this.lines = { ...lines, object: lineObject };

    this.host.scene().add(lineObject, mesh, auraObject);
    this.dirty = true;
  }

  private allocateDots(count: number): void {
    const T = this.three;
    if (this.dots) {
      this.host.scene().remove(this.dots.object);
      this.dots.geometry.dispose();
    }
    const geometry = new T.BufferGeometry();
    const attribute = (items: number) => new T.BufferAttribute(new Float32Array(count * items), items);
    const dots = { geometry, position: attribute(3), color: attribute(3), size: attribute(1), alpha: attribute(1), capacity: count };
    geometry.setAttribute("position", dots.position);
    geometry.setAttribute("aColor", dots.color);
    geometry.setAttribute("aSize", dots.size);
    geometry.setAttribute("aAlpha", dots.alpha);
    geometry.setDrawRange(0, 0);
    const object = new T.Points(geometry, this.dotMaterial);
    object.frustumCulled = false;
    this.dots = { ...dots, object };
    this.host.scene().add(object);
  }

  // Layout

  /** The reveal only runs while effects are on, motion is allowed and this scope visit has not finished one. */
  private wantReveal(): boolean {
    return this.options.effects && !this.options.reducedMotion && !this.revealDone;
  }

  /** Glow, dimming and flow wait for the reveal to end, so it reads as "finding the connection". */
  private glowOn(): boolean {
    return this.options.effects && (this.revealDone || this.options.reducedMotion);
  }

  private signature(view: SceneView): string {
    let hash = 5381;
    const feed = (text: string) => {
      for (let i = 0; i < text.length; i++) hash = ((hash << 5) + hash + text.charCodeAt(i)) | 0;
    };
    for (const n of view.nodes) feed(n.id);
    for (const l of view.links) feed(`${endOf(l.source)}>${endOf(l.target)}`);
    return `${view.nodes.length}:${view.links.length}:${hash}`;
  }

  private scheduleLayout(): void {
    if (this.layoutTimer) return;
    this.layoutTimer = window.setTimeout(
      () => {
        this.layoutTimer = 0;
        this.requestLayout();
      },
      this.options.loading ? LAYOUT_WAIT_LOADING_MS : LAYOUT_WAIT_MS,
    );
  }

  /** Asks for positions of the view as it is now; an answer for an older view is laid out again when it lands. */
  private requestLayout(): void {
    if (this.disposed || this.pending) return;
    const view = this.view;
    const count = view.nodes.length;
    if (count === 0) return;
    const at = new Map(view.nodes.map((n, i) => [n.id, i]));
    const seed = new Float32Array(count * 3).fill(Number.NaN);
    const jitter = () => (Math.random() - 0.5) * 8;
    view.nodes.forEach((n, i) => {
      const own = this.byId.get(n.id);
      // A new node starts beside its parent, so the layout grows the graph instead of reshuffling it.
      const near = own ?? this.byId.get(this.parents.get(n.id) ?? "");
      if (!near) return;
      const spread = own ? 0 : 1;
      seed.set([near.toX + spread * jitter(), near.toY + spread * jitter(), near.toZ + spread * jitter()], i * 3);
    });
    const pairs: number[] = [];
    for (const l of view.links) {
      const [s, t] = [at.get(endOf(l.source)), at.get(endOf(l.target))];
      if (s !== undefined && t !== undefined && s !== t) pairs.push(s, t);
    }
    const request: LayoutRequest = { job: ++this.job, count, seed, links: Uint32Array.from(pairs) };
    this.pending = { request, ids: view.nodes.map((n) => n.id), signature: this.signature(view), scope: this.scope };
    this.hooks.onBusy(true);
    this.post(request);
  }

  private post(request: LayoutRequest): void {
    if (this.worker) {
      this.worker.postMessage(request);
      return;
    }
    void import("./graphLayout.ts")
      .then(({ layout }) => this.onAnswer({ job: request.job, positions: layout(request) }))
      .catch((error) => {
        // A layout that cannot be computed must not hold every later request behind it.
        console.error("graph layout failed", error);
        if (this.disposed || this.pending?.request.job !== request.job) return;
        this.pending = undefined;
        this.hooks.onBusy(false);
      });
  }

  private onAnswer({ job, positions }: LayoutAnswer): void {
    const pending = this.pending;
    if (this.disposed || !pending || pending.request.job !== job) return;
    this.pending = undefined;
    this.hooks.onBusy(false);
    if (pending.scope === this.scope) this.place(pending.ids, positions, pending.signature);
    // The view moved on while this was computed: lay out what is on screen now.
    if (this.view.nodes.length > 0 && this.signature(this.view) !== this.laidOut) this.requestLayout();
  }

  /** Positions landed: nodes on screen glide to their new place, new ones grow out of their parents. */
  private place(ids: string[], positions: Float32Array, signature: string): void {
    const now = performance.now();
    const reveal = this.wantReveal();
    const motion = !this.options.reducedMotion;
    const current = new Map(this.view.nodes.map((n) => [n.id, n]));
    const deepest = Math.max(0, ...this.levels.values());
    const fresh: { item: Item; level: number }[] = [];
    ids.forEach((id, i) => {
      const node = current.get(id);
      if (!node) return;
      const [toX, toY, toZ] = [positions[i * 3], positions[i * 3 + 1], positions[i * 3 + 2]];
      const known = this.byId.get(id);
      if (known) {
        Object.assign(known, { fromX: known.x, fromY: known.y, fromZ: known.z, toX, toY, toZ, start: now, span: motion ? MOVE_MS : 0 });
        return;
      }
      const item: Item = { node, x: toX, y: toY, z: toZ, fromX: toX, fromY: toY, fromZ: toZ, toX, toY, toZ, start: now, span: 0, born: now, grow: 0, shown: 0, size: 1, rgb: [0, 0, 0], aura: 0, auraAlpha: 0, auraRgb: [0, 0, 0] };
      fresh.push({ item, level: this.levels.get(id) ?? 0 });
    });
    // Parents first: a child starts where its parent is at the moment the child is born.
    fresh.sort((a, b) => a.level - b.level);
    for (const { item, level } of fresh) {
      const grow = !motion ? 0 : reveal ? GROW_MS : LATE_GROW_MS;
      const born = now + (reveal ? revealDelay(level, deepest) : 0);
      const parent = this.byId.get(this.parents.get(item.node.id) ?? "");
      if (parent && grow > 0) {
        const t = moved(parent, born);
        const [x, y, z] = [parent.fromX + (parent.toX - parent.fromX) * t, parent.fromY + (parent.toY - parent.fromY) * t, parent.fromZ + (parent.toZ - parent.fromZ) * t];
        Object.assign(item, { x, y, z, fromX: x, fromY: y, fromZ: z });
      }
      Object.assign(item, { born, grow, start: born, span: grow });
      this.items.push(item);
      this.byId.set(item.node.id, item);
    }
    this.laidOut = signature;
    for (const it of this.items) this.animUntil = Math.max(this.animUntil, it.start + it.span, it.born + it.grow);
    if (this.items.length > this.nodeCapacity || this.view.links.length > this.linkCapacity) {
      const grown = (need: number, have: number) => (need > have ? Math.max(need, have * 2) : have);
      this.allocate(grown(this.items.length, this.nodeCapacity), grown(this.view.links.length, this.linkCapacity));
    }
    this.connect();
    this.restyle();
    this.fitWhenReady();
  }

  /** The camera frames the whole graph once per scope visit, again as the rest of a first load arrives; a refresh keeps the person's view. */
  private fitWhenReady(): void {
    if (!this.fitPending || this.items.length <= 1) return;
    const targets = new Float32Array(this.items.length * 3);
    this.items.forEach((it, i) => targets.set([it.toX, it.toY, it.toZ], i * 3));
    const { center, radius } = boundsOf(targets, this.items.length);
    const camera = this.host.camera();
    const distance = fitDistance(radius + 12, camera.fov, camera.aspect);
    const from = this.host.cameraPosition();
    let [dx, dy, dz] = [from.x - center[0], from.y - center[1], from.z - center[2]];
    const length = Math.hypot(dx, dy, dz);
    if (length < 1e-6) [dx, dy, dz] = [0, 0, 1];
    else [dx, dy, dz] = [dx / length, dy / length, dz / length];
    const look = { x: center[0], y: center[1], z: center[2] };
    this.host.cameraPosition({ x: look.x + dx * distance, y: look.y + dy * distance, z: look.z + dz * distance }, look, this.options.reducedMotion ? 0 : 700);
    if (fitSettled({ loading: this.options.loading, busy: !!this.pending || !!this.layoutTimer, placed: this.laidOut, current: this.signature(this.view) })) this.fitPending = false;
  }

  // Style

  /** The links whose two ends are on screen, in buffer order. */
  private connect(): void {
    const seen = new Set<string>();
    this.segments = [];
    for (const link of this.view.links) {
      const source = this.byId.get(endOf(link.source));
      const target = this.byId.get(endOf(link.target));
      const key = `${endOf(link.source)}|${endOf(link.target)}|${link.kind}`;
      if (!source || !target || seen.has(key) || this.segments.length >= this.linkCapacity) continue;
      seen.add(key);
      this.segments.push({ link, source, target, rgb: [0, 0, 0], dots: 0 });
    }
    this.dirty = true;
  }

  /** What every node and link is painted with under the current switches, theme and glow. */
  private restyle(): void {
    const { effects, particles, reducedMotion, colorBy, dark } = this.options;
    const theme: GraphTheme = dark ? "dark" : "light";
    const glow = this.glowOn();
    const dimmed = glow && hasGlow(this.view);
    for (const it of this.items) {
      const own = this.linear(nodeColor(it.node, this.pal, glow, false, baseColor(it.node, colorBy, theme, this.pal)));
      // While something glows the rest steps back toward the background.
      it.rgb = dimmed && !it.node.glow ? mix(this.background, own, this.pal.dim) : own;
      it.size = nodeSize(it.node, glow);
      const aura = nodeAura(it.node, effects, glow);
      it.aura = it.size * aura.reach * 2;
      // Light adds up on a dark canvas; on a light one it is laid over, so it has to be fainter.
      it.auraAlpha = aura.alpha * (dark ? 1 : 0.5) * (dimmed && !it.node.glow ? this.pal.dim : 1);
      it.auraRgb = glow && it.node.glow === "path" ? this.linear(this.pal.path) : own;
    }
    let dots = 0;
    for (const segment of this.segments) {
      const lit = glow && segment.link.glow === true;
      const own = this.linear(linkColor(segment.link, this.pal, glow, false));
      segment.rgb = lit ? own : mix(this.background, own, LINK_STRENGTH * (dimmed ? this.pal.dim : 1));
      segment.dots = linkParticles(segment.link, glow, particles && !reducedMotion);
      dots += segment.dots;
    }
    if (dots > (this.dots?.capacity ?? 0)) this.allocateDots(Math.max(64, dots * 2));
    const blending = dark ? this.three.AdditiveBlending : this.three.NormalBlending;
    for (const material of [this.auraMaterial, this.dotMaterial]) {
      if (material.blending === blending) continue;
      material.blending = blending;
      material.needsUpdate = true;
    }
    this.auraMaterial.uniforms.uPulse.value = effects && !reducedMotion ? 0.08 : 0;
    this.dirty = true;
  }

  // Frames

  private readonly tick = (now: number): void => {
    if (this.disposed) return;
    this.raf = requestAnimationFrame(this.tick);
    const moving = now < this.animUntil;
    const frame = frameWrite(moving, this.dirty, this.settling);
    this.settling = frame.owed;
    if (frame.write) {
      this.dirty = false;
      this.write(now);
    }
    if (!moving && !this.revealDone && this.items.length > 1 && !this.options.loading && !this.pending && !this.layoutTimer) {
      // The reveal ends once everything has landed: glow and flow come on.
      this.revealDone = true;
      this.restyle();
    }
    const camera = this.host.camera();
    const scale = this.host.renderer().domElement.height / (2 * Math.tan((camera.fov * Math.PI) / 360));
    this.auraMaterial.uniforms.uScale.value = scale;
    this.auraMaterial.uniforms.uTime.value = now / 1000;
    this.dotMaterial.uniforms.uScale.value = scale;
    this.flow(now);
    if (this.pointer?.moved) {
      this.pointer.moved = false;
      if (!this.press) this.setHovered(this.pick(this.pointer.x, this.pointer.y), this.pointer.x, this.pointer.y);
    }
  };

  /** Every node's and link's place for this frame, into the buffers. Runs per frame while anything moves, so it allocates nothing. */
  private write(now: number): void {
    const { mesh, auras, lines, background } = this;
    if (!mesh || !auras || !lines || !mesh.instanceColor) return;
    const matrices = mesh.instanceMatrix.array;
    const colors = mesh.instanceColor.array;
    const [auraAt, auraColor, auraSize, auraAlpha] = [auras.position.array, auras.color.array, auras.size.array, auras.alpha.array];
    const count = Math.min(this.items.length, this.nodeCapacity);
    for (let i = 0; i < count; i++) {
      const it = this.items[i];
      const t = moved(it, now);
      it.x = it.fromX + (it.toX - it.fromX) * t;
      it.y = it.fromY + (it.toY - it.fromY) * t;
      it.z = it.fromZ + (it.toZ - it.fromZ) * t;
      it.shown = it.grow <= 0 ? 1 : easeOut((now - it.born) / it.grow);
      const s = it.size * it.shown;
      // A scale and a translation: the other twelve entries of the matrix stay zero.
      const m = i * 16;
      matrices[m] = matrices[m + 5] = matrices[m + 10] = s;
      matrices[m + 12] = it.x;
      matrices[m + 13] = it.y;
      matrices[m + 14] = it.z;
      matrices[m + 15] = 1;
      const c = i * 3;
      colors[c] = it.rgb[0];
      colors[c + 1] = it.rgb[1];
      colors[c + 2] = it.rgb[2];
      auraAt[c] = it.x;
      auraAt[c + 1] = it.y;
      auraAt[c + 2] = it.z;
      auraColor[c] = it.auraRgb[0];
      auraColor[c + 1] = it.auraRgb[1];
      auraColor[c + 2] = it.auraRgb[2];
      auraSize[i] = it.aura * it.shown;
      auraAlpha[i] = it.auraAlpha * it.shown;
      this.radii[i] = it.shown > 0.5 ? Math.max(it.size * 1.5, 3) : 0;
    }
    mesh.count = count;
    mesh.instanceMatrix.needsUpdate = true;
    mesh.instanceColor.needsUpdate = true;
    auras.geometry.setDrawRange(0, count);
    auras.position.needsUpdate = auras.color.needsUpdate = auras.size.needsUpdate = auras.alpha.needsUpdate = true;

    const [lineAt, lineColor] = [lines.position.array, lines.color.array];
    const segments = Math.min(this.segments.length, this.linkCapacity);
    for (let j = 0; j < segments; j++) {
      const { source, target, rgb } = this.segments[j];
      const p = j * 6;
      lineAt[p] = source.x;
      lineAt[p + 1] = source.y;
      lineAt[p + 2] = source.z;
      lineAt[p + 3] = target.x;
      lineAt[p + 4] = target.y;
      lineAt[p + 5] = target.z;
      // A link appears with the later of its two ends.
      const shown = Math.min(source.shown, target.shown);
      for (let k = 0; k < 3; k++) lineColor[p + k] = lineColor[p + 3 + k] = background[k] + (rgb[k] - background[k]) * shown;
    }
    lines.geometry.setDrawRange(0, segments * 2);
    lines.position.needsUpdate = true;
    lines.color.needsUpdate = true;
  }

  /** The dots flowing along glowing links, source to target. */
  private flow(now: number): void {
    const dots = this.dots;
    if (!dots) return;
    const glow = this.linear(this.pal.linkGlow);
    const [at, color, size, alpha] = [dots.position.array, dots.color.array, dots.size.array, dots.alpha.array];
    let n = 0;
    for (const { source, target, dots: count } of this.segments) {
      for (let k = 0; k < count && n < dots.capacity; k++, n++) {
        const t = ((now / 1000) * FLOW_SPEED + k / count) % 1;
        const p = n * 3;
        at[p] = source.x + (target.x - source.x) * t;
        at[p + 1] = source.y + (target.y - source.y) * t;
        at[p + 2] = source.z + (target.z - source.z) * t;
        color[p] = glow[0];
        color[p + 1] = glow[1];
        color[p + 2] = glow[2];
        size[n] = 4.5;
        alpha[n] = Math.min(source.shown, target.shown);
      }
    }
    dots.geometry.setDrawRange(0, n);
    if (n > 0) dots.position.needsUpdate = dots.color.needsUpdate = dots.size.needsUpdate = dots.alpha.needsUpdate = true;
  }

  // Pointer

  private pick(x: number, y: number): Item | undefined {
    const { clientWidth: width, clientHeight: height } = this.el;
    if (!this.auras || width === 0 || height === 0) return undefined;
    const camera = this.host.camera();
    this.raycaster.setFromCamera(new this.three.Vector2((x / width) * 2 - 1, -(y / height) * 2 + 1), camera);
    const { origin, direction } = this.raycaster.ray;
    const hit = pickSphere(origin, direction, this.auras.position.array, this.radii, Math.min(this.items.length, this.nodeCapacity));
    return hit < 0 ? undefined : this.items[hit];
  }

  private setHovered(item: Item | undefined, x = 0, y = 0): void {
    if (!item && !this.hovered) return;
    this.hovered = item;
    this.el.style.cursor = item ? "pointer" : "";
    this.hooks.onHover(item?.node, x, y);
  }

  private at(event: PointerEvent): { x: number; y: number } {
    const rect = this.el.getBoundingClientRect();
    return { x: event.clientX - rect.left, y: event.clientY - rect.top };
  }

  private readonly onPointerDown = (event: PointerEvent): void => {
    if (event.button !== 0) return;
    this.press = this.at(event);
    // Dragging turns the camera: the tooltip would only trail behind it.
    this.setHovered(undefined);
  };

  private readonly onPointerUp = (event: PointerEvent): void => {
    const press = this.press;
    this.press = undefined;
    if (!press || event.button !== 0) return;
    const { x, y } = this.at(event);
    // A press that moved was a camera drag, not a click.
    if (Math.hypot(x - press.x, y - press.y) > 5) return;
    const item = this.pick(x, y);
    if (item) this.hooks.onClick(item.node);
  };

  private readonly onPointerMove = (event: PointerEvent): void => {
    this.pointer = { ...this.at(event), moved: true };
  };

  private readonly onPointerLeave = (): void => {
    this.pointer = undefined;
    this.press = undefined;
    this.setHovered(undefined);
  };
}
