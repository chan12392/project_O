/**
 * What a file or area node is painted with under each 색 기준 (ADR-0033): a pure module
 * so the rules are testable without a browser. Every category carries a dark- and a
 * light-theme variant for contrast on the app background; glow always overrides them —
 * it stays the strongest signal.
 */
import type { GraphColorBy, GraphNode, GraphView } from "../../platform/backend/types.ts";
import type { GraphPalette } from "./graphStyle.ts";

export type GraphTheme = "dark" | "light";

type LangGroup = "blue" | "orange" | "slate" | "yellow" | "pink" | "green" | "cyan" | "lime" | "red";

const LANG_DARK: Record<LangGroup, string> = {
  blue: "#58a6ff",
  orange: "#f78166",
  slate: "#8b949e",
  yellow: "#d29922",
  pink: "#f778ba",
  green: "#3fb950",
  cyan: "#39c5cf",
  lime: "#9ece6a",
  red: "#ff7b72",
};

const LANG_LIGHT: Record<LangGroup, string> = {
  blue: "#0969da",
  orange: "#bc4c00",
  slate: "#57606a",
  yellow: "#9a6700",
  pink: "#bf3989",
  green: "#1a7f37",
  cyan: "#0a7ea4",
  lime: "#5c7d0e",
  red: "#cf222e",
};

const EXT_GROUP: Record<string, LangGroup> = {
  ts: "blue",
  tsx: "blue",
  js: "blue",
  jsx: "blue",
  mjs: "blue",
  cjs: "blue",
  rs: "orange",
  md: "slate",
  mdx: "slate",
  json: "yellow",
  yaml: "yellow",
  yml: "yellow",
  toml: "yellow",
  css: "pink",
  scss: "pink",
  py: "green",
  go: "cyan",
  sh: "lime",
  ps1: "lime",
  html: "red",
};

/** A fixed categorical ramp for 영역별: a stable hue per top-level folder. */
const AREA_DARK = ["#f778ba", "#79c0ff", "#56d364", "#e3b341", "#d2a8ff", "#ffa198", "#39c5cf", "#9ece6a"];
const AREA_LIGHT = ["#bf3989", "#0969da", "#1a7f37", "#9a6700", "#8250df", "#cf222e", "#0a7ea4", "#5c7d0e"];

/** A file node's extension from its project path (or its label as a fallback); "" when none. */
export function extOf(node: Pick<GraphNode, "path" | "label">): string {
  const name = node.path ?? node.label;
  const base = name.split("/").pop() ?? name;
  const dot = base.lastIndexOf(".");
  return dot > 0 ? base.slice(dot + 1).toLowerCase() : "";
}

/** The language color for an extension, or undefined for one outside the known groups. */
export function languageColor(ext: string, theme: GraphTheme): string | undefined {
  const group = EXT_GROUP[ext];
  return group ? (theme === "light" ? LANG_LIGHT : LANG_DARK)[group] : undefined;
}

/** A name → one slot of the categorical ramp, stable across views (djb2). */
function areaIndex(name: string): number {
  let hash = 5381;
  for (let i = 0; i < name.length; i++) hash = ((hash << 5) + hash + name.charCodeAt(i)) | 0;
  return Math.abs(hash) % AREA_DARK.length;
}

/**
 * The top-level folder a file or area belongs to: the first segment of its project path.
 * `crates/o-core` and `src/core` share a hue with `crates` and `src`, and the same folder
 * keeps the same hue in the overall and the project views.
 */
export function topAreaOf(node: Pick<GraphNode, "id" | "path">): string {
  const path = node.path ?? (node.id.startsWith("a:") ? (node.id.split("|")[1] ?? "") : "");
  return path.split("/").filter(Boolean)[0] ?? "";
}

/** The categorical color for a top-level folder, or undefined for an empty name. */
export function areaColor(area: string, theme: GraphTheme): string | undefined {
  return area ? (theme === "light" ? AREA_LIGHT : AREA_DARK)[areaIndex(area)] : undefined;
}

/** A node's base color before glow and dimming; kinds without a category keep the palette's kind tone. */
export function baseColor(node: Pick<GraphNode, "id" | "kind" | "path" | "label">, colorBy: GraphColorBy, theme: GraphTheme, pal: Pick<GraphPalette, "root" | "project" | "area" | "file">): string {
  if (colorBy === "area") {
    if (node.kind === "file" || node.kind === "area") return areaColor(topAreaOf(node), theme) ?? pal[node.kind];
    return pal[node.kind];
  }
  // Areas keep their own hue in both modes: the overall graph has no files, and one blue for every area told nothing apart.
  if (node.kind === "area") return areaColor(topAreaOf(node), theme) ?? pal.area;
  return node.kind === "file" ? (languageColor(extOf(node), theme) ?? pal.file) : pal[node.kind];
}

/** One legend row: a swatch and what it stands for. */
export interface LegendItem {
  label: string;
  color: string;
}

/** The categories actually on screen — the legend never lists a color nothing uses. */
export function legendItems(view: Pick<GraphView, "nodes">, colorBy: GraphColorBy, theme: GraphTheme, neutral: string): LegendItem[] {
  // No files on screen (the overall graph): 언어별 has nothing to list, so show the area hues the nodes do carry.
  if (colorBy === "language" && !view.nodes.some((n) => n.kind === "file")) return legendItems(view, "area", theme, neutral);
  const counts = new Map<string, { item: LegendItem; n: number }>();
  const bump = (label: string, color: string) => {
    const hit = counts.get(label) ?? { item: { label, color }, n: 0 };
    hit.n++;
    counts.set(label, hit);
  };
  for (const node of view.nodes) {
    if (colorBy === "language") {
      if (node.kind !== "file") continue;
      const ext = extOf(node);
      bump(ext ? `.${ext}` : "기타", (ext && languageColor(ext, theme)) || neutral);
    } else {
      if (node.kind !== "file" && node.kind !== "area") continue;
      const area = topAreaOf(node);
      if (area) bump(area, areaColor(area, theme) ?? neutral);
    }
  }
  return [...counts.values()]
    .sort((a, b) => b.n - a.n)
    .slice(0, 8)
    .map(({ item }) => item);
}
