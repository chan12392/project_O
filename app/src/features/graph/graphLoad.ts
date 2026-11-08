/**
 * The graph view loader behind `useGraphView` (ADR-0033): pure so the merge rules and
 * the stale-answer guard are testable without React. The overall scope no longer waits
 * for one `graphView`: the skeleton drawn from the project list shows right away, then
 * each `graphOverallPart` answer merges in as it lands (at most 4 in flight). A refresh
 * only applies while its scope key is still on screen (#81).
 */
import type { Backend, GraphLink, GraphNode, GraphView } from "../../platform/backend/types.ts";

/** Which graph is on screen: every project ("overall") or one folder. */
export type GraphScope = { scope: "overall" } | { scope: "project"; project: string };

/** The scope's state key — what a refresh answer must still match to apply (#81). */
export const keyOf = (scope: GraphScope) => (scope.scope === "overall" ? "overall" : scope.project);

/** The least a listed project needs for the skeleton and its part request. */
export interface ListedProject {
  path: string;
  name: string;
  missing?: boolean;
}

export interface LoaderState {
  /** The scope this answer belongs to; a stale one never shows. */
  key?: string;
  view?: GraphView;
  error?: string;
  loading: boolean;
}

/** A link end as the backend sends it (a node id) or after the library resolves it. */
const endOf = (end: string | { id: string }): string => (typeof end === "string" ? end : end.id);

/**
 * The overall skeleton drawn the moment the scope opens: the root node and one node per
 * listed project — `indexed` unknown, so neutral until the project's part answers.
 */
export function skeletonView(projects: ListedProject[]): GraphView {
  const nodes: GraphNode[] = [
    { id: "o:root", label: "Project O", kind: "root", size: Math.max(1, projects.length) },
    ...projects.map((p): GraphNode => ({ id: `p:${p.path}`, label: p.name, kind: "project", project: p.path, size: 1 })),
  ];
  const links: GraphLink[] = projects.map((p) => ({ source: "o:root", target: `p:${p.path}`, kind: "contains", weight: 1 }));
  return { scope: "overall", grafly: { installed: true }, indexed: false, nodes, links, truncated: false };
}

/**
 * The overall view on screen with a placeholder (and root edge) for each listed project it
 * does not have yet: the list can arrive after the Graph tab opened, and a reload must not
 * drop the nodes already grown.
 */
export function withSkeleton(view: GraphView, projects: ListedProject[]): GraphView {
  const skeleton = skeletonView(projects);
  const have = new Set(view.nodes.map((n) => n.id));
  const missing = skeleton.nodes.filter((n) => !have.has(n.id));
  if (missing.length === 0) return view;
  const added = new Set(missing.map((n) => n.id));
  return { ...view, nodes: [...view.nodes, ...missing], links: [...view.links, ...skeleton.links.filter((l) => added.has(endOf(l.target)))] };
}

/**
 * One project's part merged in: its node and areas replace the placeholder (and any older
 * slice of the same project on a refresh); the root→project edge stays.
 */
export function mergePart(base: GraphView, part: GraphView, project: string): GraphView {
  const pid = `p:${project}`;
  const dropped = new Set(base.nodes.filter((n) => n.project === project).map((n) => n.id));
  // The part's nodes land where the project's old slice sat, so the order stays stable.
  const at = Math.max(0, base.nodes.findIndex((n) => dropped.has(n.id)));
  const nodes = [...base.nodes.filter((n) => !dropped.has(n.id))];
  nodes.splice(at, 0, ...part.nodes.filter((n) => n.id !== "o:root"));
  const ids = new Set(nodes.map((n) => n.id));
  const seen = new Set<string>();
  const links = [...base.links.filter((l) => !dropped.has(endOf(l.source)) && !dropped.has(endOf(l.target))), ...part.links].filter((l) => {
    const s = endOf(l.source);
    const t = endOf(l.target);
    const key = `${s}|${t}|${l.kind}`;
    if (!ids.has(s) || !ids.has(t) || seen.has(key)) return false;
    seen.add(key);
    return true;
  });
  // The part carries only the project's own edges; the root→project edge comes from the skeleton.
  if (ids.has("o:root") && ids.has(pid) && !links.some((l) => endOf(l.source) === "o:root" && endOf(l.target) === pid)) {
    links.push({ source: "o:root", target: pid, kind: "contains", weight: 1 });
  }
  return {
    scope: "overall",
    grafly: part.grafly,
    indexed: nodes.some((n) => n.kind === "project" && n.indexed === true),
    nodes,
    links,
    truncated: base.truncated || part.truncated,
    deep: part.deep ?? base.deep,
  };
}

/** A failed part lands on its own project node (the detail panel reads `error`), never on the whole view. */
export function markProjectError(view: GraphView, project: string, message: string): GraphView {
  return { ...view, nodes: view.nodes.map((n) => (n.id === `p:${project}` ? { ...n, error: message } : n)) };
}

/**
 * Each node's depth in the contains tree: roots are 0, and every other node sits one
 * level under its contains parent (cycles and orphans stay at 0 — they appear first).
 */
export function levelsOf(view: Pick<GraphView, "nodes" | "links">): Map<string, number> {
  const parent = new Map<string, string>();
  for (const l of view.links) {
    if (l.kind !== "contains") continue;
    const t = endOf(l.target);
    if (!parent.has(t)) parent.set(t, endOf(l.source));
  }
  const levels = new Map<string, number>();
  const levelOf = (id: string, trail: ReadonlySet<string>): number => {
    const known = levels.get(id);
    if (known !== undefined) return known;
    const p = parent.get(id);
    const level = p === undefined || trail.has(id) ? 0 : levelOf(p, new Set(trail).add(id)) + 1;
    levels.set(id, level);
    return level;
  };
  for (const n of view.nodes) levelOf(n.id, new Set());
  return levels;
}

export interface LoaderDeps {
  /** The two calls the loader needs; a test can fake just these. */
  api: Pick<Backend, "graphView" | "graphOverallPart">;
  /** The scope on screen now — read at call time, so a captured refresh can't aim at a stale scope (#81). */
  scope(): GraphScope;
  /** The listed projects, for the overall skeleton and its part requests. */
  projects(): ListedProject[];
  /** Apply a state change; functional, like React's setState. */
  emit(apply: (s: LoaderState) => LoaderState): void;
}

/** Part requests run at most this many at a time. */
export const PART_POOL = 4;

const textOf = (e: unknown) => (e instanceof Error ? e.message : String(e));

export function createViewLoader(deps: LoaderDeps): { refresh(expectedKey?: string): Promise<void> } {
  let seq = 0;

  /** One `graphView` answer for a project scope, or the whole overall graph as the fallback. */
  async function loadWhole(mine: number, key: string, project?: string) {
    try {
      const view = await deps.api.graphView(project);
      if (seq === mine) deps.emit(() => ({ key, view, loading: false }));
    } catch (e) {
      if (seq === mine) deps.emit((s) => ({ key, view: s.key === key ? s.view : undefined, error: textOf(e), loading: false }));
    }
  }

  /** The overall graph assembled from parts: skeleton first, each project merged as it lands. */
  async function loadParts(mine: number, key: string, projects: ListedProject[]) {
    const part = deps.api.graphOverallPart;
    if (!part) return loadWhole(mine, key);
    deps.emit((s) => ({ key, view: s.key === key && s.view?.scope === "overall" ? withSkeleton(s.view, projects) : skeletonView(projects), loading: true, error: undefined }));
    const targets = projects.filter((p) => !p.missing);
    let failed = 0;
    let next = 0;
    const worker = async () => {
      while (next < targets.length && seq === mine) {
        const p = targets[next++];
        try {
          const slice = await part.call(deps.api, p.path);
          if (seq !== mine) return;
          deps.emit((s) => (s.key === key && s.view ? { ...s, view: mergePart(s.view, slice, p.path) } : s));
        } catch (e) {
          if (seq !== mine) return;
          failed++;
          const message = textOf(e);
          deps.emit((s) => (s.key === key && s.view ? { ...s, view: markProjectError(s.view, p.path, message) } : s));
        }
      }
    };
    await Promise.all(Array.from({ length: Math.min(PART_POOL, targets.length) }, worker));
    if (seq !== mine) return;
    // Every part failing means the backend cannot slice the graph (old `o`): one whole view, as before.
    if (targets.length > 0 && failed === targets.length) return loadWhole(mine, key);
    deps.emit((s) => (s.key === key ? { ...s, loading: false } : s));
  }

  return {
    /**
     * Loads the current scope. `expectedKey` (the key a build/Deep started on) must still
     * match: a stale refresh then does nothing — it never bumps the sequence nor stores a
     * view under the wrong key, so the current scope's own load is left untouched (#81).
     */
    async refresh(expectedKey?: string) {
      const scope = deps.scope();
      const key = keyOf(scope);
      if (expectedKey !== undefined && expectedKey !== key) return;
      const mine = ++seq;
      if (scope.scope === "project") {
        deps.emit((s) => ({ ...s, loading: true }));
        return loadWhole(mine, key, scope.project);
      }
      return loadParts(mine, key, deps.projects());
    },
  };
}
