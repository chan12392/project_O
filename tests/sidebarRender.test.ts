import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { test } from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";

import type { RunView } from "../src/core/snapshot.ts";

const fromApp = createRequire(new URL("../app/package.json", import.meta.url));
const load = (id: string) => import(pathToFileURL(fromApp.resolve(id)).href);
const [{ createServer }, { default: react }, { createElement }, { renderToStaticMarkup }] = await Promise.all([
  load("vite"), load("@vitejs/plugin-react"), load("react"), load("react-dom/server"),
]);

test("the rendered sidebar groups recorded branches, filters automation and keeps the project run limit", async () => {
  const server = await createServer({ root: fileURLToPath(new URL("../app", import.meta.url)), configFile: false, appType: "custom", optimizeDeps: { noDiscovery: true, include: [] }, server: { middlewareMode: true, hmr: false }, plugins: [react()] });
  const originalStorage = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
  let view: unknown = {};
  Object.defineProperty(globalThis, "localStorage", { configurable: true, value: { getItem: (key: string) => key === "project-o-sidebar-view" ? JSON.stringify(view) : null } });
  try {
    const { Sidebar } = await server.ssrLoadModule("/src/shell/Sidebar.tsx");
    const project = "C:/work/app";
    const run = (runId: string, branch?: string, scheduled?: boolean): RunView => ({
      runId, title: `run-${runId}`, project, branch, scheduled,
      startedAt: "2026-10-01T01:00:00Z", endedAt: "2026-10-01T02:00:00Z", status: "done", cards: [], messages: [], quotas: [],
    });
    const runs = [run("first", "younchan-task"), run("legacy"), run("automatic", "younchan-task", true), run("older", "younchan")];
    const noop = () => {};
    const props = {
      state: "full", projects: [{ path: project, name: "app", kind: "registered", runs }],
      groups: { groups: [], member: {} }, leadState: () => "resting", project, run: "legacy", now: new Date("2026-11-12T12:00:00Z"),
      onProject: noop, onWake: noop, onRun: noop, onCard: noop, onNewRequest: noop,
      onSearch: noop, onDashboard: noop, onAutomation: noop, onDreaming: noop, onListed: noop, onUnregister: noop,
    };
    const render = () => renderToStaticMarkup(createElement(Sidebar, props)) as string;
    const initial = render();
    assert.match(initial, /aria-label="워크스페이스 옵션"/);
    assert.match(initial, /aria-label="younchan-task 브랜치 실행"/);
    assert.match(initial, /aria-label="브랜치 정보 없음"/);
    assert.match(initial, /aria-current="true"[^>]*>[\s\S]*?run-legacy/);
    assert.match(initial, /이전 실행 1개/);
    assert.doesNotMatch(initial, /run-older/);
    assert.equal((initial.match(/aria-label="younchan-task 브랜치 실행"/g) ?? []).length, 1);

    view = { hideScheduled: true };
    const filtered = render();
    assert.doesNotMatch(filtered, /run-automatic|이전 실행/);
    assert.match(filtered, /run-older/);

    view = { runLimit: 0 };
    const folded = render();
    assert.match(folded, /이전 실행 4개/);
    assert.doesNotMatch(folded, /run-first|run-legacy|run-automatic|run-older/);

    view = { runLimit: "all" };
    const all = render();
    for (const { title } of runs) assert.ok(all.includes(title), title);
    assert.doesNotMatch(all, /이전 실행/);
  } finally {
    if (originalStorage) Object.defineProperty(globalThis, "localStorage", originalStorage);
    else Reflect.deleteProperty(globalThis, "localStorage");
    await server.close();
  }
});
