import assert from "node:assert/strict";
import { test } from "node:test";

import type { GraphView } from "../app/src/platform/backend/types.ts";
import {
  capOverall,
  createViewLoader,
  levelsOf,
  markProjectError,
  mergePart,
  skeletonView,
  withSkeleton,
  type GraphScope,
  type ListedProject,
  type LoaderState,
} from "../app/src/features/graph/graphLoad.ts";

const PROJECTS: ListedProject[] = [
  { path: "/p/a", name: "a" },
  { path: "/p/b", name: "b" },
];

const partOf = (project: string, areas: string[] = ["src"], indexed = true): GraphView => ({
  scope: "overall",
  grafly: { installed: true, version: "0.4.0" },
  indexed,
  truncated: false,
  nodes: [
    { id: `p:${project}`, label: project.split("/").pop()!, kind: "project", project, size: 10, indexed },
    ...areas.map((a) => ({ id: `a:${project}|${a}`, label: a, kind: "area" as const, project, path: a, size: 5 })),
  ],
  links: areas.map((a) => ({ source: `p:${project}`, target: `a:${project}|${a}`, kind: "contains" as const, weight: 1 })),
});

test("the skeleton draws root plus one neutral node per listed project, linked from the root", () => {
  const view = skeletonView(PROJECTS);
  assert.equal(view.scope, "overall");
  assert.equal(view.nodes[0].id, "o:root");
  assert.equal(view.nodes[0].label, "Project O");
  assert.deepEqual(
    view.nodes.filter((n) => n.kind === "project").map((n) => n.id),
    ["p:/p/a", "p:/p/b"],
  );
  // Indexed is unknown — neutral, not a false "인덱스 없음".
  assert.ok(!("indexed" in view.nodes[1]));
  assert.equal(view.links.length, 2);
  assert.ok(view.links.every((l) => l.source === "o:root"));
});

test("mergePart replaces the project placeholder with its real node and areas, keeping others", () => {
  const merged = mergePart(skeletonView(PROJECTS), partOf("/p/a"), "/p/a");
  // The real nodes land where the placeholder sat, the other project keeps its slot.
  assert.deepEqual(
    merged.nodes.map((n) => n.id),
    ["o:root", "p:/p/a", "a:/p/a|src", "p:/p/b"],
  );
  assert.equal(merged.nodes[1].indexed, true);
  // Root→project edge survives, and the part's contains edge came along.
  assert.ok(merged.links.some((l) => l.source === "o:root" && l.target === "p:/p/a"));
  assert.ok(merged.links.some((l) => l.source === "p:/p/a" && l.target === "a:/p/a|src"));
  assert.equal(merged.indexed, true);

  // A refresh of the same project drops stale areas instead of duplicating them.
  const again = mergePart(merged, partOf("/p/a", ["app"]), "/p/a");
  assert.deepEqual(
    again.nodes.filter((n) => n.project === "/p/a").map((n) => n.id),
    ["p:/p/a", "a:/p/a|app"],
  );
  assert.ok(!again.links.some((l) => l.target === "a:/p/a|src"));
});

test("markProjectError lands the failure on the project node only", () => {
  const marked = markProjectError(skeletonView(PROJECTS), "/p/a", "boom");
  assert.equal(marked.nodes[1].error, "boom");
  assert.equal(marked.nodes[2].error, undefined);
});

test("levelsOf walks the contains tree: root 0, each level one under its parent", () => {
  const view = mergePart(skeletonView(PROJECTS), partOf("/p/a"), "/p/a");
  const levels = levelsOf(view);
  assert.equal(levels.get("o:root"), 0);
  assert.equal(levels.get("p:/p/a"), 1);
  assert.equal(levels.get("a:/p/a|src"), 2);
  // A cycle never recurses forever: x's parent is y, y's parent is x → x lands at 2 via y.
  const cyclic: GraphView = {
    scope: "project",
    grafly: { installed: true },
    indexed: true,
    truncated: false,
    nodes: [
      { id: "d:x", label: "x", kind: "area", path: "x", size: 1 },
      { id: "d:y", label: "y", kind: "area", path: "y", size: 1 },
    ],
    links: [
      { source: "d:x", target: "d:y", kind: "contains", weight: 1 },
      { source: "d:y", target: "d:x", kind: "contains", weight: 1 },
    ],
  };
  const l = levelsOf(cyclic);
  assert.equal(l.get("d:x"), 2);
  assert.equal(l.get("d:y"), 1);
});

interface FakeApi {
  graphView: (project?: string) => Promise<GraphView>;
  graphOverallPart?: (project: string) => Promise<GraphView>;
  graphSettings?: () => Promise<{ maxNodes: number }>;
}

/** A loader wired like the hook: mutable scope/projects, emits applied to a state var. */
function rig(projects: ListedProject[], api: FakeApi, scope: GraphScope = { scope: "overall" }) {
  const r = {
    scope,
    state: { loading: false } as LoaderState,
    emits: 0,
    loader: undefined as unknown as ReturnType<typeof createViewLoader>,
  };
  r.loader = createViewLoader({
    api: { graphSettings: async () => ({ maxNodes: 1500 }), ...api },
    scope: () => r.scope,
    projects: () => projects,
    emit: (apply) => {
      r.emits++;
      r.state = apply(r.state);
    },
  });
  return r;
}

const deferred = <T>() => {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
};

/** Let requests and their async continuations settle without applying queued updaters. */
const settle = () => new Promise<void>((resolve) => setImmediate(resolve));

test("a project scope loads its one view and a stale answer is dropped", async () => {
  const calls: (string | undefined)[] = [];
  const pending = new Map<string | undefined, ReturnType<typeof deferred<GraphView>>>();
  const api = {
    graphView: (project?: string) => {
      calls.push(project);
      const d = deferred<GraphView>();
      pending.set(project, d);
      return d.promise;
    },
  };
  const r = rig(PROJECTS, api, { scope: "project", project: "/p/a" });
  const first = r.loader.refresh();
  // The screen moves to another project before A answers.
  r.scope = { scope: "project", project: "/p/b" };
  const second = r.loader.refresh();
  pending.get("/p/b")!.resolve(partOf("/p/b"));
  await second;
  pending.get("/p/a")!.resolve(partOf("/p/a"));
  await first;
  assert.equal(r.state.key, "/p/b");
  assert.equal(r.state.view?.nodes[0].id, "p:/p/b");
  assert.equal(r.state.loading, false);
});

test("a refresh for a scope that is no longer current does nothing at all (#81)", async () => {
  let called = 0;
  const api = {
    graphView: async () => {
      called++;
      return partOf("/p/b");
    },
  };
  const r = rig(PROJECTS, api, { scope: "project", project: "/p/b" });
  // A build for project /p/a finished after the person moved to /p/b: its captured
  // refresh must not bump the sequence, store anything, or call the backend.
  await r.loader.refresh("/p/a");
  assert.equal(r.emits, 0);
  assert.equal(called, 0);
  assert.equal(r.state.loading, false);
});

test("the overall scope draws the skeleton at once and merges each part as it lands, four in flight", async () => {
  const projects = ["a", "b", "c", "d", "e", "f"].map((n) => ({ path: `/p/${n}`, name: n }));
  const pending = new Map<string, ReturnType<typeof deferred<GraphView>>>();
  let inFlight = 0;
  let maxInFlight = 0;
  const api = {
    graphView: async () => {
      throw new Error("whole view must not be needed");
    },
    graphOverallPart: (project: string) => {
      inFlight++;
      maxInFlight = Math.max(maxInFlight, inFlight);
      const d = deferred<GraphView>();
      pending.set(project, d);
      return d.promise.finally(() => inFlight--);
    },
  };
  const r = rig(projects, api);
  const done = r.loader.refresh();
  assert.equal(r.state.loading, true);
  await settle();
  // The skeleton uses the saved cap before any project answer lands.
  assert.equal(r.state.view?.nodes.length, 7);
  assert.equal(inFlight, 4, "at most four part requests in flight");
  /** Waits until a project's part request has been issued, then answers it. */
  const answer = async (project: string) => {
    while (!pending.has(project)) await Promise.resolve();
    pending.get(project)!.resolve(partOf(project));
  };
  await answer("/p/a");
  await answer("/p/c");
  await answer("/p/e");
  await answer("/p/b");
  await answer("/p/d");
  await answer("/p/f");
  await done;
  assert.ok(maxInFlight <= 4);
  assert.equal(r.state.loading, false);
  assert.equal(r.state.view?.nodes.length, 7 + 6, "each project's area merged in");
  assert.equal(r.state.error, undefined);
});

test("a failed part lands on its project node, not as a global error; a missing folder is never asked", async () => {
  const projects: ListedProject[] = [
    { path: "/p/a", name: "a" },
    { path: "/p/gone", name: "gone", missing: true },
    { path: "/p/b", name: "b" },
  ];
  const asked: string[] = [];
  const api = {
    graphView: async () => {
      throw new Error("not reached: some parts succeed");
    },
    graphOverallPart: async (project: string) => {
      asked.push(project);
      if (project === "/p/a") throw new Error("a broke");
      return partOf(project);
    },
  };
  const r = rig(projects, api);
  await r.loader.refresh();
  assert.deepEqual(asked, ["/p/a", "/p/b"], "missing folders are skipped");
  const nodeA = r.state.view?.nodes.find((n) => n.id === "p:/p/a");
  assert.equal(nodeA?.error, "a broke");
  assert.equal(r.state.error, undefined);
  assert.equal(r.state.loading, false);
  // The surviving part still merged its areas in.
  assert.ok(r.state.view?.nodes.some((n) => n.id === "a:/p/b|src"));
});

test("when every part fails the whole view is the fallback (old `o` or no slice)", async () => {
  const whole = partOf("/p/a");
  const api = {
    graphView: async () => whole,
    graphOverallPart: async () => {
      throw new Error("o: unknown command 'part'");
    },
  };
  const r = rig(PROJECTS, api);
  await r.loader.refresh();
  assert.equal(r.state.view, whole);
  assert.equal(r.state.loading, false);

  // A backend without the part call goes straight to the whole view.
  const r2 = rig(PROJECTS, { graphView: async () => whole });
  await r2.loader.refresh();
  assert.equal(r2.state.view, whole);
});

test("a project list that arrives after the overall graph opened grows into it without dropping what is there", () => {
  const early = skeletonView([]);
  assert.deepEqual(early.nodes.map((n) => n.id), ["o:root"]);
  const grown = mergePart(withSkeleton(early, [{ path: "C:/p/a", name: "a" }]), { scope: "overall", grafly: { installed: true }, indexed: true, truncated: false,
    nodes: [{ id: "p:C:/p/a", label: "a", kind: "project", project: "C:/p/a", size: 2, indexed: true }, { id: "a:C:/p/a|src", label: "src", kind: "area", project: "C:/p/a", size: 2 }],
    links: [{ source: "p:C:/p/a", target: "a:C:/p/a|src", kind: "contains", weight: 1 }] }, "C:/p/a");
  const later = withSkeleton(grown, [{ path: "C:/p/a", name: "a" }, { path: "C:/p/b", name: "b" }]);
  assert.deepEqual(later.nodes.map((n) => n.id), ["o:root", "p:C:/p/a", "a:C:/p/a|src", "p:C:/p/b"]);
  assert.ok(later.links.some((l) => l.source === "o:root" && l.target === "p:C:/p/b"));
  assert.equal(withSkeleton(later, [{ path: "C:/p/b", name: "b" }]), later, "nothing missing: the same view");
});

test("queued and replayed initial updaters preserve the first answered project (#101)", async () => {
  let state: LoaderState = { loading: false };
  const queue: ((s: LoaderState) => LoaderState)[] = [];
  const pending = new Map<string, ReturnType<typeof deferred<GraphView>>>();
  const loader = createViewLoader({
    api: {
      graphSettings: async () => ({ maxNodes: 1500 }),
      graphView: async () => { throw new Error("parts must succeed"); },
      graphOverallPart: (project) => {
        const response = deferred<GraphView>();
        pending.set(project, response);
        return response.promise;
      },
    },
    scope: () => ({ scope: "overall" }),
    projects: () => PROJECTS,
    emit: (apply) => queue.push(apply),
  });
  const flush = () => {
    for (const apply of queue.splice(0)) {
      const before = structuredClone(state);
      const first = apply(state);
      assert.deepEqual(state, before, "an updater must not mutate its previous state");
      assert.deepEqual(apply(state), first, "StrictMode replay produces the same transition");
      state = first;
    }
  };

  // Repeat on a refresh with the opposite response order, retaining the raw state.
  for (const order of [["/p/a", "/p/b"], ["/p/b", "/p/a"]]) {
    pending.clear();
    const done = loader.refresh();
    await settle();
    pending.get(order[0])!.resolve(partOf(order[0], ["first"]));
    await settle();
    assert.ok(queue.length >= 2, "the initial and first-answer updaters are still queued");
    flush();
    assert.ok(state.view?.nodes.some((n) => n.id === `a:${order[0]}|first`));
    pending.get(order[1])!.resolve(partOf(order[1], ["second"]));
    await done;
    flush();
    for (const [i, project] of order.entries()) {
      const id = `a:${project}|${i === 0 ? "first" : "second"}`;
      assert.ok(state.view?.nodes.some((n) => n.id === id), `${project}'s displayed area survives`);
      assert.ok(state.raw?.nodes.some((n) => n.id === id), `${project}'s original area survives`);
      assert.equal(state.raw?.nodes.find((n) => n.id === `p:${project}`)?.indexed, true);
    }
    assert.equal(state.loading, false);
  }
});

const eightProjects = Array.from({ length: 8 }, (_, i) => ({ path: `/p/${i}`, name: `${i}` }));
const twelveAreas = Array.from({ length: 12 }, (_, i) => `area-${i}`);
const assertClosedLinks = (view: GraphView) => {
  const ids = new Set(view.nodes.map((n) => n.id));
  assert.ok(view.links.every((l) => ids.has(l.source) && ids.has(l.target)), "every link endpoint survives the cap");
};

test("8 × 13 project nodes obey maxNodes=100 throughout split loading (#84)", async () => {
  const snapshots: LoaderState[] = [];
  const pending = new Map<string, ReturnType<typeof deferred<GraphView>>>();
  let state: LoaderState = { loading: false };
  const loader = createViewLoader({
    api: {
      graphSettings: async () => ({ maxNodes: 100 }),
      graphView: async () => { throw new Error("whole view must not be needed"); },
      graphOverallPart: (project) => {
        const response = deferred<GraphView>();
        pending.set(project, response);
        return response.promise;
      },
    },
    scope: () => ({ scope: "overall" }),
    projects: () => eightProjects,
    emit: (apply) => { state = apply(state); snapshots.push(state); },
  });
  const done = loader.refresh();
  // Answer each wave in reverse order rather than in the project-list order.
  for (const i of [3, 2, 1, 0, 7, 6, 5, 4]) {
    await settle();
    const project = `/p/${i}`;
    assert.ok(pending.has(project), `${project} is in flight`);
    pending.get(project)!.resolve(partOf(project, twelveAreas));
  }
  await done;
  assert.equal(state.raw?.nodes.length, 105, "all original parts remain available for recapping");
  assert.equal(state.raw?.truncated, false, "display clipping does not contaminate the original graph");
  assert.equal(state.view?.nodes.length, 100);
  assert.equal(state.view?.truncated, true);
  for (const s of snapshots) {
    if (!s.view) continue;
    assert.ok(s.view.nodes.length <= 100, "intermediate views obey the saved limit");
    assertClosedLinks(s.view);
  }
});

test("a refresh recaps retained originals with the newly saved limit before new parts answer", async () => {
  let maxNodes = 100;
  let hold = false;
  const pending: ReturnType<typeof deferred<GraphView>>[] = [];
  const r = rig(eightProjects, {
    graphSettings: async () => ({ maxNodes }),
    graphView: async () => { throw new Error("parts must succeed"); },
    graphOverallPart: async (project) => {
      if (hold) {
        const response = deferred<GraphView>();
        pending.push(response);
        await response.promise;
      }
      return partOf(project, twelveAreas);
    },
  });
  await r.loader.refresh();
  assert.equal(r.state.view?.nodes.length, 100);

  hold = true;
  maxNodes = 200;
  const raised = r.loader.refresh();
  await settle();
  assert.equal(r.state.raw?.nodes.length, 105);
  assert.equal(r.state.view?.nodes.length, 105, "raising the cap restores clipped original nodes immediately");
  assert.equal(r.state.view?.truncated, false);
  hold = false;
  for (const response of pending.splice(0)) response.resolve(partOf("unused"));
  await raised;

  maxNodes = 100;
  await r.loader.refresh();
  assert.equal(r.state.raw?.nodes.length, 105);
  assert.equal(r.state.view?.nodes.length, 100);
  assert.equal(r.state.view?.truncated, true);
});

test("a failed refresh part retains its original nodes while the display remains capped", async () => {
  let fail = false;
  const r = rig(eightProjects, {
    graphSettings: async () => ({ maxNodes: 100 }),
    graphView: async () => { throw new Error("partial success must not fall back"); },
    graphOverallPart: async (project) => {
      if (fail && project === "/p/7") throw new Error("temporarily unavailable");
      return partOf(project, twelveAreas);
    },
  });
  await r.loader.refresh();
  fail = true;
  await r.loader.refresh();
  assert.equal(r.state.raw?.nodes.length, 105);
  assert.equal(r.state.raw?.nodes.find((n) => n.id === "p:/p/7")?.error, "temporarily unavailable");
  assert.equal(r.state.view?.nodes.length, 100);
  assert.equal(r.state.view?.truncated, true);
  assertClosedLinks(r.state.view!);
});

test("split loading and the capped whole-view fallback retain the same graph", async () => {
  const original = eightProjects.reduce((view, project) => mergePart(view, partOf(project.path, twelveAreas), project.path), skeletonView(eightProjects));
  const whole = capOverall(original, 100);
  const split = rig(eightProjects, {
    graphSettings: async () => ({ maxNodes: 100 }),
    graphView: async () => { throw new Error("parts must succeed"); },
    graphOverallPart: async (project) => partOf(project, twelveAreas),
  });
  await split.loader.refresh();
  const fallback = rig(eightProjects, {
    graphSettings: async () => ({ maxNodes: 100 }),
    graphView: async () => whole,
    graphOverallPart: async () => { throw new Error("part unavailable"); },
  });
  await fallback.loader.refresh();
  assert.deepEqual(split.state.view, fallback.state.view);

  // A failed settings read must use the backend's capped endpoint, rather than
  // guessing a cap and rendering uncapped parts.
  let parts = 0;
  const failedSettings = rig(eightProjects, {
    graphSettings: async () => { throw new Error("settings unavailable"); },
    graphView: async () => whole,
    graphOverallPart: async (project) => { parts++; return partOf(project, twelveAreas); },
  });
  await failedSettings.loader.refresh();
  assert.equal(parts, 0);
  assert.equal(failedSettings.state.view, whole);
  assert.equal(failedSettings.state.loading, false);
});

test("capOverall matches the Rust cap_overall_matches_frontend_contract table", () => {
  const view: GraphView = {
    scope: "overall", grafly: { installed: true }, indexed: true, truncated: false,
    nodes: [
      { id: "o:root", label: "root", kind: "root", size: 1 },
      { id: "a:orphan", label: "orphan", kind: "area", project: "/missing", size: 1 },
      { id: "p:/cold", label: "cold", kind: "project", project: "/cold", size: 1 },
      { id: "a:/cold|plain", label: "plain", kind: "area", project: "/cold", size: 1 },
      { id: "a:/hot|active", label: "active", kind: "area", project: "/hot", glow: "active", size: 1 },
      { id: "a:/hot|path", label: "path", kind: "area", project: "/hot", glow: "path", size: 1 },
      { id: "p:/hot", label: "hot", kind: "project", project: "/hot", size: 1 },
      { id: "a:/hot|plain", label: "plain", kind: "area", project: "/hot", size: 1 },
      { id: "p:/last", label: "last", kind: "project", project: "/last", size: 1 },
    ],
    links: [
      { source: "o:root", target: "p:/cold", kind: "contains", weight: 1 },
      { source: "o:root", target: "p:/hot", kind: "contains", weight: 1 },
      { source: "o:root", target: "p:/last", kind: "contains", weight: 1 },
      { source: "p:/cold", target: "a:/cold|plain", kind: "contains", weight: 1 },
      { source: "p:/hot", target: "a:/hot|active", kind: "contains", weight: 1 },
      { source: "p:/hot", target: "a:/hot|path", kind: "contains", weight: 1 },
      { source: "p:/hot", target: "a:/hot|plain", kind: "contains", weight: 1 },
      { source: "p:/missing", target: "a:orphan", kind: "contains", weight: 1 },
      { source: "a:/cold|plain", target: "a:/hot|active", kind: "calls", weight: 1 },
    ],
  };
  const cases: [number, string[]][] = [
    [2, ["o:root", "a:/hot|active", "a:/hot|path", "p:/hot"]],
    [5, ["o:root", "p:/cold", "a:/hot|active", "a:/hot|path", "p:/hot"]],
    [6, ["o:root", "p:/cold", "a:/cold|plain", "a:/hot|active", "a:/hot|path", "p:/hot"]],
    [7, ["o:root", "p:/cold", "a:/cold|plain", "a:/hot|active", "a:/hot|path", "p:/hot", "a:/hot|plain"]],
    [8, ["o:root", "p:/cold", "a:/cold|plain", "a:/hot|active", "a:/hot|path", "p:/hot", "a:/hot|plain", "p:/last"]],
    [100, ["o:root", "p:/cold", "a:/cold|plain", "a:/hot|active", "a:/hot|path", "p:/hot", "a:/hot|plain", "p:/last"]],
  ];
  const original = structuredClone(view);
  for (const [maxNodes, expected] of cases) {
    const capped = capOverall(view, maxNodes);
    assert.deepEqual(capped.nodes.map((n) => n.id), expected, `maxNodes=${maxNodes}`);
    assert.equal(capped.truncated, true);
    const ids = new Set(expected);
    assert.deepEqual(capped.links, view.links.filter((l) => ids.has(l.source) && ids.has(l.target)));
    assertClosedLinks(capped);
  }
  assert.deepEqual(view, original, "capping never mutates its original graph");
  const rootOnly = { ...view, nodes: view.nodes.slice(0, 1), links: [], truncated: true };
  assert.equal(capOverall(rootOnly, 100).truncated, true, "backend truncation remains visible");
});
