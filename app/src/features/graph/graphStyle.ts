/**
 * What the 3D graph paints with (ADR-0033). Pure: `Graph3D` reads the app's CSS variables
 * once per mount and hands every accessor a palette built here, so the same rules are
 * testable without a browser and both themes just work.
 */
import type { GraphLink, GraphNode, GraphView } from "../../platform/backend/types.ts";
import { finite } from "./graphText.ts";

export interface GraphPalette {
  /** Node colors by kind. */
  root: string;
  project: string;
  area: string;
  file: string;
  /** Glow overrides: "active" (changed now) and "path" (its impact path). */
  active: string;
  path: string;
  /** Link colors, plain and on a glowing path. */
  link: string;
  linkGlow: string;
  /** Non-glowing nodes/links fade to this much while something glows (0–1). */
  dim: number;
}

/** The canvas colors, resolved from CSS variables on the graph's element. */
export function palette(read: (name: string) => string): GraphPalette {
  const at = (name: string, fallback: string) => {
    const value = read(name).trim();
    return value || fallback;
  };
  return {
    root: at("--foreground", "#f0f0f0"),
    project: at("--chart-2", "#3b82f6"),
    area: at("--chart-4", "#1d4ed8"),
    file: at("--muted-foreground", "#acacac"),
    active: at("--agent-working", "#eab308"),
    path: at("--status-warning", "#eab308"),
    link: at("--border", "#404040"),
    linkGlow: at("--agent-working", "#eab308"),
    // Mild dimming: non-glowing nodes keep their color readable instead of near-invisible.
    dim: 0.7,
  };
}

/** A color faded back while something else glows: "color-mix(in srgb, <c> <dim>%, transparent)". */
function fade(color: string, palette: GraphPalette): string {
  return `color-mix(in srgb, ${color} ${Math.round(palette.dim * 100)}%, transparent)`;
}

/** `base` is the 색 기준 color for the node (language/area); without it the kind color stands. */
export function nodeColor(node: Pick<GraphNode, "kind" | "glow">, p: GraphPalette, effects: boolean, dimmed: boolean, base?: string): string {
  // Only the changed files take the glow color; a path node keeps its language/area color and
  // glows through its halo and the flow, so a long path does not paint the whole graph one color.
  const color = effects && node.glow === "active" ? p.active : (base ?? p[node.kind]);
  return effects && dimmed && !node.glow ? fade(color, p) : color;
}

/**
 * A node's sphere radius: kinds read as depth levels, `size` nudges within a level.
 * With effects, glowing nodes grow so the changed file and its path stand out.
 */
export function nodeSize(node: Pick<GraphNode, "kind" | "size" | "glow">, effects: boolean): number {
  const base = { root: 7, project: 5.5, area: 4, file: 2.4 }[node.kind];
  const size = base + Math.min(2.4, Math.log2(Math.max(1, finite(node.size) ? node.size : 0)) * 0.4);
  if (!effects) return size;
  return node.glow === "active" ? size * 1.7 : node.glow === "path" ? size * 1.3 : size;
}

/** The halo a glowing node gets with effects on (a soft sprite behind the sphere), 0 for none. */
export function nodeHalo(node: Pick<GraphNode, "glow">, effects: boolean): number {
  return effects && node.glow === "active" ? 2.2 : effects && node.glow === "path" ? 1.4 : 0;
}

export function linkColor(link: Pick<GraphLink, "glow">, p: GraphPalette, effects: boolean, dimmed: boolean): string {
  if (effects && link.glow) return p.linkGlow;
  return effects && dimmed ? fade(p.link, p) : p.link;
}

/** Dots flowing along a glowing link; both switches and the link's glow must be on. */
export function linkParticles(link: Pick<GraphLink, "weight" | "glow">, effects: boolean, particles: boolean): number {
  return effects && particles && link.glow ? Math.min(6, 2 + (finite(link.weight) ? link.weight : 0)) : 0;
}

/** Whether the view has anything glowing: while it does, the rest fades back. */
export function hasGlow(view: Pick<GraphView, "nodes" | "links">): boolean {
  return view.nodes.some((n) => n.glow) || view.links.some((l) => l.glow);
}
