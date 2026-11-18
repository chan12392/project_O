import assert from "node:assert/strict";
import { test } from "node:test";

import type { Project } from "../app/src/data/projects.ts";
import {
  byStatus,
  DEFAULT_VIEW,
  fullOrder,
  isScheduled,
  parseView,
  placeAt,
  projectStatus,
  runsByBranch,
  RUN_LIMITS,
  shownRuns,
  sortProjects,
  STATUS_WORD,
  withoutScheduled,
  type SidebarView,
} from "../app/src/data/sidebarView.ts";
import { folderKey } from "../src/core/projects.ts";
import type { CardView, RunView } from "../src/core/snapshot.ts";

const NOW = new Date("2026-11-10T12:00:00.000Z");
/** A time this many minutes before NOW. */
const ago = (minutes: number) => new Date(NOW.getTime() - minutes * 60_000).toISOString();
const HOUR = 60;
const DAY = 24 * HOUR;

const card = (id: string, state: CardView["state"], over: Partial<CardView> = {}): CardView => ({
  id,
  provider: "claude",
  label: id,
  state,
  activity: "",
  log: [],
  usage: { input: 0, cached: 0, output: 0 },
  ...over,
});

/** A run that ended cleanly an hour ago unless told otherwise. */
const run = (runId: string, over: Partial<RunView> = {}): RunView => ({
  runId,
  title: runId,
  project: "C:/work/app",
  startedAt: ago(2 * HOUR),
  endedAt: ago(HOUR),
  status: "done",
  cards: [],
  messages: [],
  quotas: [],
  ...over,
});

/** A run still going, with one card that wrote something a minute ago. */
const running = (runId: string, over: Partial<RunView> = {}): RunView =>
  run(runId, { status: "running", endedAt: undefined, cards: [card("a", "running", { startedAt: ago(1) })], ...over });

const project = (path: string, over: Partial<Project> = {}): Project => ({
  path,
  name: path.split("/").pop() ?? path,
  kind: "registered",
  runs: [],
  ...over,
});

const names = (projects: readonly Project[]) => projects.map((p) => p.name);

test("runs group by recorded branch, retaining order and an explicit unknown group", () => {
  const runs = [run("a", { branch: "younchan-task" }), run("old"), run("b", { branch: "younchan" }), run("c", { branch: "younchan-task" }), run("legacy")];
  const grouped = runsByBranch(runs);
  assert.deepEqual(grouped.map((g) => [g.branch, g.runs.map((r) => r.runId)]), [
    ["younchan-task", ["a", "c"]], [undefined, ["old", "legacy"]], ["younchan", ["b"]],
  ]);
  assert.equal(grouped[0].runs[0], runs[0]);
  assert.deepEqual(runsByBranch([]), []);
  assert.deepEqual(runs.map((r) => r.runId), ["a", "old", "b", "c", "legacy"]);
});

const VALID: SidebarView = { groupBy: "status", sortBy: "manual", order: ["C:/work/app", "C:/work/web"], hideScheduled: true, runLimit: 5 };

test("the default view is the sidebar as it was before it had options", () => {
  assert.deepEqual(DEFAULT_VIEW, { groupBy: "group", sortBy: "recent", order: [], hideScheduled: false, runLimit: 3 });
});

test("a saved view that is no object reads as the default view", () => {
  for (const saved of [null, undefined, "status", 3, true, ["status"], [VALID]]) {
    assert.deepEqual(parseView(saved), DEFAULT_VIEW, `saved as ${JSON.stringify(saved)}`);
  }
  assert.deepEqual(parseView({}), DEFAULT_VIEW, "an object with no choices");
});

test("a valid saved view comes back as it was saved", () => {
  assert.deepEqual(parseView(VALID), VALID);
  assert.deepEqual(parseView(JSON.parse(JSON.stringify(VALID))), VALID, "through storage");
  assert.deepEqual(parseView({ ...VALID, theme: "dark" }), VALID, "a key the view does not have is dropped");
  // The falsy choices are choices too.
  const plain = { groupBy: "none", sortBy: "name", order: [], hideScheduled: false, runLimit: 0 };
  assert.deepEqual(parseView(plain), plain);
  assert.equal(parseView({ ...VALID, runLimit: "all" }).runLimit, "all");
});

test("one invalid choice in a saved view goes back to its default and the others are kept", () => {
  const broken: [keyof SidebarView, unknown][] = [
    ["groupBy", "bogus"],
    ["groupBy", undefined],
    ["sortBy", 3],
    ["sortBy", "Recent"],
    ["runLimit", 4],
    ["runLimit", "3"],
    ["runLimit", null],
    ["hideScheduled", "yes"],
    ["hideScheduled", 1],
    ["order", "C:/work/app"],
    ["order", { 0: "C:/work/app" }],
  ];
  for (const [field, value] of broken) {
    assert.deepEqual(parseView({ ...VALID, [field]: value }), { ...VALID, [field]: DEFAULT_VIEW[field] }, `${field}: ${JSON.stringify(value)}`);
  }
  const { sortBy: _, ...missing } = VALID;
  assert.deepEqual(parseView(missing), { ...VALID, sortBy: DEFAULT_VIEW.sortBy }, "a missing choice");
});

test("a saved manual order keeps only its paths", () => {
  const saved = { ...VALID, order: ["C:/work/app", 3, "", null, ["C:/work/shop"], "C:/work/web"] };
  assert.deepEqual(parseView(saved), VALID);
});

test("only a run marked scheduled: true counts as started by an automation", () => {
  assert.equal(isScheduled(run("r1", { scheduled: true })), true);
  assert.equal(isScheduled(run("r2", { scheduled: false })), false);
  assert.equal(isScheduled(run("r3")), false);
});

test("hiding scheduled runs drops only them and leaves the other projects as the same objects", () => {
  const mixed = project("C:/work/app", { lastAt: ago(HOUR), runs: [run("auto", { scheduled: true }), run("manual", { scheduled: false }), run("plain")] });
  const plain = project("C:/work/web", { runs: [run("w1"), run("w2", { scheduled: false })] });
  const empty = project("C:/work/shop");
  const input = [mixed, plain, empty];
  const out = withoutScheduled(input);

  assert.deepEqual(out.map((p) => p.runs.map((r) => r.runId)), [["manual", "plain"], ["w1", "w2"], []]);
  assert.equal(out[1], plain, "no scheduled run: the very same project");
  assert.equal(out[2], empty);
  assert.notEqual(out[0], mixed);
  assert.deepEqual({ ...out[0], runs: [] }, { ...mixed, runs: [] }, "the rest of the project is kept");
  // The input is left alone.
  assert.deepEqual(input, [mixed, plain, empty]);
  assert.deepEqual(mixed.runs.map((r) => r.runId), ["auto", "manual", "plain"]);

  const onlyAuto = withoutScheduled([project("C:/work/blog", { runs: [run("nightly", { scheduled: true })] })]);
  assert.deepEqual(onlyAuto.map((p) => [p.path, p.runs.length]), [["C:/work/blog", 0]], "the project stays listed without its runs");
  assert.deepEqual(withoutScheduled([]), []);
});

test("a run limit says how many run rows show before the fold", () => {
  assert.deepEqual([...RUN_LIMITS], [0, 3, 5, "all"]);
  assert.deepEqual(RUN_LIMITS.map(shownRuns), [0, 3, 5, Infinity]);
});

test("sorting by recent puts the newest activity first and quiet projects after, by name", () => {
  const input = [
    project("C:/work/app", { lastAt: "2026-11-01T09:00:00.000Z" }),
    project("C:/work/zeta"),
    project("C:/work/blog", { lastAt: "2026-11-03T09:00:00.000Z" }),
    project("C:/work/alpha"),
    project("C:/work/web", { lastAt: "2026-11-02T09:00:00.000Z" }),
    // The same moment as web: the name decides.
    project("C:/work/shop", { lastAt: "2026-11-02T09:00:00.000Z" }),
  ];
  const before = [...input];
  const sorted = sortProjects(input, "recent");
  assert.deepEqual(names(sorted), ["blog", "shop", "web", "app", "alpha", "zeta"]);
  assert.deepEqual(input, before, "the input keeps its order");
  assert.notEqual(sorted, input);
  assert.deepEqual(sortProjects(sorted, "recent"), sorted, "a list already in order comes back in it");
  assert.deepEqual(sortProjects([], "recent"), []);
});

test("sorting by name follows the Korean locale whatever the activity", () => {
  const input = [
    project("C:/work/blog", { lastAt: "2026-11-09T09:00:00.000Z" }),
    project("C:/work/Zeta", { lastAt: "2026-11-08T09:00:00.000Z" }),
    project("C:/work/나무", { lastAt: "2026-11-01T09:00:00.000Z" }),
    project("C:/work/App"),
    project("C:/work/가게"),
  ];
  const before = [...input];
  // Hangul before Latin, and case does not split the Latin names ("App" < "blog" < "Zeta").
  assert.deepEqual(names(sortProjects(input, "name")), ["가게", "나무", "App", "blog", "Zeta"]);
  assert.deepEqual(input, before, "the input keeps its order");

  // Two folders with one name: the path decides, so the order does not depend on activity.
  const twins = [project("D:/work/app", { lastAt: "2026-11-09T09:00:00.000Z" }), project("C:/work/app")];
  assert.deepEqual(sortProjects(twins, "name").map((p) => p.path), ["C:/work/app", "D:/work/app"]);
});

test("the manual order names projects like folders; the ones it does not know follow, newest first", () => {
  const input = [
    project("C:/work/shop", { lastAt: "2026-11-01T09:00:00.000Z" }),
    project("C:/work/docs"),
    project("c:/work/app", { lastAt: "2026-11-02T09:00:00.000Z" }),
    project("C:/work/blog", { lastAt: "2026-11-05T09:00:00.000Z" }),
    project("C:/work/web", { lastAt: "2026-11-09T09:00:00.000Z" }),
  ];
  const before = [...input];
  // "gone" is in the order but not listed; it takes no place.
  const order = ["C:\\Work\\App\\", "C:/work/gone", "C:/work/web"];
  assert.deepEqual(names(sortProjects(input, "manual", order)), ["app", "web", "blog", "shop", "docs"]);
  assert.deepEqual(input, before, "the input keeps its order");
  assert.deepEqual(order, ["C:\\Work\\App\\", "C:/work/gone", "C:/work/web"]);

  // Without an order (or with none given) nothing is known: the recent order.
  assert.deepEqual(sortProjects(input, "manual", []), sortProjects(input, "recent"));
  assert.deepEqual(sortProjects(input, "manual"), sortProjects(input, "recent"));
});

test("a path written twice in the manual order keeps its first place", () => {
  const input = [project("C:/work/web"), project("C:/work/app"), project("C:/work/shop")];
  assert.deepEqual(names(sortProjects(input, "manual", ["C:/work/app", "C:/work/web", "c:\\work\\app"])), ["app", "web", "shop"]);
  assert.deepEqual(names(sortProjects(input, "manual", ["C:/work/shop", "C:/work/shop", "C:/work/web"])), ["shop", "web", "app"]);
});

const A = "C:/work/app";
const B = "C:/work/blog";
const C = "C:/work/web";

test("a project is placed right before or after another in the manual order", () => {
  const order = [A, B, C];
  assert.deepEqual(placeAt(order, order, C, A, "before"), [C, A, B]);
  assert.deepEqual(placeAt(order, order, A, C, "after"), [B, C, A]);
  assert.deepEqual(placeAt(order, order, A, B, "after"), [B, A, C]);
  assert.deepEqual(placeAt(order, order, C, B, "before"), [A, C, B]);
  // Already there: nothing moves.
  assert.deepEqual(placeAt(order, order, A, B, "before"), [A, B, C]);
  assert.deepEqual(placeAt(order, order, B, A, "after"), [A, B, C]);
  assert.deepEqual(order, [A, B, C], "the order given is left alone");
});

test("projects the order does not know join its end before the move, so an empty order starts from the list", () => {
  const listed = [A, B, C];
  assert.deepEqual(placeAt([], listed, C, A, "before"), [C, A, B]);
  assert.deepEqual(placeAt([], listed, A, B, "after"), [B, A, C]);
  assert.deepEqual(placeAt([B], listed, C, B, "before"), [C, B, A]);
  assert.deepEqual(placeAt([B], listed, A, B, "before"), [A, B, C]);
  assert.deepEqual(listed, [A, B, C], "the list given is left alone");

  // What the sidebar does on the first drag: the list as shown, with the one project moved.
  const projects = [project(A, { lastAt: "2026-11-03T09:00:00.000Z" }), project(B, { lastAt: "2026-11-02T09:00:00.000Z" }), project(C, { lastAt: "2026-11-01T09:00:00.000Z" })];
  const shown = sortProjects(projects, "recent").map((p) => p.path);
  assert.deepEqual(sortProjects(projects, "manual", placeAt([], shown, C, A, "before")).map((p) => p.path), [C, A, B]);
});

test("choosing the manual order takes in every project shown, after the ones it already has", () => {
  assert.deepEqual(fullOrder([], [A, B, C]), [A, B, C]);
  assert.deepEqual(fullOrder([], [B, C, A]), [B, C, A], "switching from name order to manual keeps the displayed order");
  assert.deepEqual(fullOrder([C, "C:/work/gone"], [A, B, C]), [C, "C:/work/gone", A, B]);
  assert.deepEqual(fullOrder(["C:\\Work\\App\\"], ["c:/work/app", B]), ["C:\\Work\\App\\", B], "a folder is taken once, as first spelled");
});

test("a path in the order that is no longer listed keeps its place through a move", () => {
  const gone = "C:/work/gone";
  assert.deepEqual(placeAt([A, gone, B], [A, B, C], C, A, "before"), [C, A, gone, B]);
  assert.deepEqual(placeAt([A, gone, B], [A, B, C], A, B, "after"), [gone, B, A, C]);
  assert.deepEqual(placeAt([gone, A], [A, B], B, A, "before"), [gone, B, A]);
});

test("a move that names no known project, or a project onto itself, changes nothing", () => {
  const full = [B, A, C];
  assert.deepEqual(placeAt([B], [A, B, C], "C:/work/nowhere", A, "before"), full, "an unknown project");
  assert.deepEqual(placeAt([B], [A, B, C], A, "C:/work/nowhere", "after"), full, "an unknown target");
  assert.deepEqual(placeAt([B], [A, B, C], A, A, "before"), full, "onto itself");
  assert.deepEqual(placeAt([B], [A, B, C], "C:\\Work\\App\\", "c:/work/app", "after"), full, "onto itself in another spelling");
  assert.deepEqual(placeAt([], [], A, B, "before"), []);
});

test("a manual order never holds a folder twice, however it is spelled", () => {
  const order = ["C:\\Work\\App\\", C, "c:/work/app", C];
  const listed = ["C:/work/app/", C, "C:/work/shop"];
  const unique = (paths: string[]) => new Set(paths.map(folderKey)).size === paths.length;

  // The first spelling stays; path and target are matched like folders.
  const moved = placeAt(order, listed, "c:\\work\\shop", "c:/work/app", "before");
  assert.deepEqual(moved, ["C:/work/shop", "C:\\Work\\App\\", C]);
  assert.ok(unique(moved));

  const unmoved = placeAt(order, listed, "C:/work/nowhere", C, "after");
  assert.deepEqual(unmoved, ["C:\\Work\\App\\", C, "C:/work/shop"]);
  assert.ok(unique(unmoved));

  assert.deepEqual(placeAt(order, listed, "C:/WORK/APP", "C:/work/shop", "after"), [C, "C:/work/shop", "C:\\Work\\App\\"]);
});

test("the three statuses have their Korean labels", () => {
  assert.deepEqual(STATUS_WORD, { active: "진행 중", attention: "확인 필요", quiet: "조용함" });
});

test("a project with an O run going is active; a Claude Code session open on this PC is not", () => {
  assert.equal(projectStatus({ runs: [running("20261110-110000-ab12")] }, NOW), "active");
  assert.equal(projectStatus({ runs: [run("20261109-090000-cd34"), running("20261110-110000-ab12")] }, NOW), "active", "any of its runs");
  assert.equal(projectStatus({ runs: [running("session-3f2a")] }, NOW), "quiet");
  assert.equal(projectStatus({ runs: [running("session-3f2a"), running("20261110-110000-ab12")] }, NOW), "active");
});

test("a project with no runs, or only runs that ended cleanly, is quiet", () => {
  assert.equal(projectStatus({ runs: [] }, NOW), "quiet");
  assert.equal(projectStatus({ runs: [run("20261110-090000-ab12", { cards: [card("a", "done", { endedAt: ago(HOUR) })] })] }, NOW), "quiet");
  assert.equal(projectStatus({ runs: [run("20261110-090000-ab12", { status: "stopped", cards: [card("a", "stopped", { endedAt: ago(HOUR) })] })] }, NOW), "quiet", "stopped by a person");
});

test("a recent run with a failed card wants a look; the same failure from over a week ago does not", () => {
  const failedAt = (minutes: number) =>
    run("20261101-090000-ab12", { status: "failed", startedAt: ago(minutes + HOUR), endedAt: ago(minutes), cards: [card("a", "failed", { endedAt: ago(minutes), result: "빌드 실패" })] });
  assert.equal(projectStatus({ runs: [failedAt(DAY)] }, NOW), "attention");
  assert.equal(projectStatus({ runs: [failedAt(6 * DAY)] }, NOW), "attention");
  assert.equal(projectStatus({ runs: [failedAt(8 * DAY)] }, NOW), "quiet");
  assert.equal(projectStatus({ runs: [failedAt(30 * DAY)] }, NOW), "quiet");
  // The old failure does not hide what the project does now.
  assert.equal(projectStatus({ runs: [running("20261110-110000-ab12"), failedAt(8 * DAY)] }, NOW), "active");
});

test("a run refused before it had any card wants a look too", () => {
  const refused = (minutes: number) => run("20261110-110000-ab12", { status: "failed", startedAt: ago(minutes + 1), endedAt: ago(minutes), blocked: [{ rule: "daily-budget", message: "오늘 예산을 다 썼습니다" }] });
  assert.equal(projectStatus({ runs: [refused(5)] }, NOW), "attention");
  assert.equal(projectStatus({ runs: [refused(8 * DAY)] }, NOW), "quiet");
});

test("a card waiting for the lead's answer wants a look; a report that asks nothing does not", () => {
  const done = card("a", "done", { endedAt: ago(HOUR) });
  const withMessages = (messages: RunView["messages"]) => ({ runs: [run("20261110-090000-ab12", { cards: [done], messages })] });
  const asks = { at: ago(90), from: "a", to: "lead", text: "어느 폴더에 둘까요?" };

  assert.equal(projectStatus(withMessages([asks]), NOW), "attention");
  assert.equal(projectStatus(withMessages([{ ...asks, id: "q1", kind: "question", text: "폴더를 정해야 합니다." }]), NOW), "attention", "sent as a question");
  // A note to read, as the badges count it.
  assert.equal(projectStatus(withMessages([{ ...asks, text: "README에 한 줄을 더했습니다." }]), NOW), "quiet");
  // The lead wrote back since.
  assert.equal(projectStatus(withMessages([asks, { at: ago(80), from: "lead", to: "a", text: "src 아래에 두세요." }]), NOW), "quiet");
  assert.equal(projectStatus(withMessages([{ at: ago(90), from: "lead", to: "a", text: "어디까지 됐나요?" }]), NOW), "quiet", "the lead's own question");
});

test("a card held by a usage limit wants a look; one that only went quiet does not", () => {
  const limited = card("a", "running", { startedAt: ago(30), log: [{ at: ago(20), kind: "limit", text: "사용량 한도: 15:00까지 차단" }] });
  assert.equal(projectStatus({ runs: [running("20261110-110000-ab12", { cards: [limited] })] }, NOW), "attention");
  // Nothing new for half an hour: the row's "조용함" chip, which the grouping leaves out.
  const silent = card("a", "running", { startedAt: ago(30) });
  assert.equal(projectStatus({ runs: [running("20261110-110000-ab12", { cards: [silent] })] }, NOW), "active");
});

test("wanting a look wins over being active", () => {
  const going = running("20261110-110000-ab12", { cards: [card("a", "running", { startedAt: ago(1) }), card("b", "failed", { endedAt: ago(5), result: "테스트 실패" })] });
  assert.equal(projectStatus({ runs: [going] }, NOW), "attention");
  // Across runs too: one run going, another that failed yesterday.
  const failed = run("20261109-090000-cd34", { status: "failed", endedAt: ago(DAY), cards: [card("a", "failed", { endedAt: ago(DAY) })] });
  assert.equal(projectStatus({ runs: [running("20261110-110000-ab12"), failed] }, NOW), "attention");
});

test("projects are grouped as active, attention, quiet, in the order they came, without empty groups", () => {
  const failed = () => run("20261109-090000-cd34", { status: "failed", endedAt: ago(DAY), cards: [card("a", "failed", { endedAt: ago(DAY) })] });
  const quiet1 = project("C:/work/docs");
  const active1 = project("C:/work/app", { runs: [running("20261110-110000-ab12")] });
  const look1 = project("C:/work/shop", { runs: [failed()] });
  const active2 = project("C:/work/web", { runs: [running("20261110-113000-ef56")] });
  const quiet2 = project("C:/work/blog", { runs: [running("session-3f2a")] });
  const look2 = project("C:/work/api", { runs: [failed()] });
  const input = [quiet1, active1, look1, active2, quiet2, look2];

  const sections = byStatus(input, NOW);
  assert.deepEqual(sections.map((s) => [s.status, names(s.projects)]), [
    ["active", ["app", "web"]],
    ["attention", ["shop", "api"]],
    ["quiet", ["docs", "blog"]],
  ]);
  assert.equal(sections[0].projects[0], active1, "the projects themselves, not copies");
  assert.deepEqual(input, [quiet1, active1, look1, active2, quiet2, look2], "the input keeps its order");

  assert.deepEqual(byStatus([quiet2, look1, quiet1], NOW).map((s) => [s.status, names(s.projects)]), [
    ["attention", ["shop"]],
    ["quiet", ["blog", "docs"]],
  ]);
  assert.deepEqual(byStatus([quiet1, quiet2], NOW).map((s) => s.status), ["quiet"]);
  assert.deepEqual(byStatus([active2], NOW).map((s) => s.status), ["active"]);
  assert.deepEqual(byStatus([], NOW), []);
});
