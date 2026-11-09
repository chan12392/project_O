import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { test } from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";

const fromApp = createRequire(new URL("../app/package.json", import.meta.url));
const load = (id) => import(pathToFileURL(fromApp.resolve(id)).href);
const [{ createServer }, { default: react }, { createElement }, { renderToStaticMarkup }] = await Promise.all([
  load("vite"), load("@vitejs/plugin-react"), load("react"), load("react-dom/server"),
]);

test("lead status warnings remain visible in the conversation", async () => {
  const server = await createServer({ root: fileURLToPath(new URL("../app", import.meta.url)), configFile: false, appType: "custom", optimizeDeps: { noDiscovery: true, include: [] }, server: { middlewareMode: true, hmr: false }, plugins: [react()] });
  try {
    globalThis.window = { location: { search: "" }, matchMedia: () => ({ matches: false, addEventListener() {} }), addEventListener() {} };
    globalThis.document = { documentElement: { classList: { toggle() {} }, style: { setProperty() {} }, dataset: {} } };
    const { LeadThread } = await server.ssrLoadModule("/src/features/chat/LeadThread.tsx");
    const { leadItems } = await server.ssrLoadModule("/../src/core/lead.ts");
    const message = "실행 상태 확인 불가: 일부 결과를 읽지 못했습니다";
    const lead = { items: leadItems([{ t: "lead.warning", at: "2026-01-01T00:00:00Z", text: message }]), loaded: true, busy: false };
    const html = renderToStaticMarkup(createElement(LeadThread, { project: "C:/project", lead, onCompose() {}, async onStartProject() {} }));
    assert.match(html, /role="status"/);
    assert.ok(html.includes(message));
  } finally {
    delete globalThis.window;
    delete globalThis.document;
    await server.close();
  }
});
