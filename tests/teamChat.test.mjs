import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { test } from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";

const fromApp = createRequire(new URL("../app/package.json", import.meta.url));
const load = (id) => import(pathToFileURL(fromApp.resolve(id)).href);
const [{ createServer }, { default: react }, { createElement }, { renderToStaticMarkup }] = await Promise.all([
  load("vite"), load("@vitejs/plugin-react"), load("react"), load("react-dom/server"),
]);

test("an approval can be answered until its matching reply is recorded", async () => {
  const server = await createServer({ root: fileURLToPath(new URL("../app", import.meta.url)), configFile: false, appType: "custom", optimizeDeps: { noDiscovery: true, include: [] }, server: { middlewareMode: true, hmr: false }, plugins: [react()] });
  try {
    const { TeamChat } = await server.ssrLoadModule("/src/features/chat/TeamChat.tsx");
    const request = { at: "2026-01-01T00:00:00Z", from: "a", to: "lead", id: "question", kind: "approval", text: "확인해 주세요" };
    const render = (messages) => renderToStaticMarkup(createElement(TeamChat, { run: { runId: "r1", cards: [], messages } }));
    assert.match(render([request]), /승인 요청에 답하기/);
    assert.match(render([request, { ...request, from: "lead", to: "a", replyTo: "another", kind: "message" }]), /승인 요청에 답하기/);
    const answered = render([request, { ...request, from: "lead", to: "a", replyTo: "question", kind: "message" }]);
    assert.doesNotMatch(answered, /승인 요청에 답하기/);
    assert.match(answered, /답변 완료/);
  } finally {
    await server.close();
  }
});
