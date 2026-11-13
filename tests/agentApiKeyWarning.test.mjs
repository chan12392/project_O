import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { test } from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";

const fromApp = createRequire(new URL("../app/package.json", import.meta.url));
const load = (id) => import(pathToFileURL(fromApp.resolve(id)).href);
const [{ createServer }, { default: react }, { createElement }, { renderToStaticMarkup }] = await Promise.all([
  load("vite"), load("@vitejs/plugin-react"), load("react"), load("react-dom/server"),
]);

test("settings shows only the agent's detected API variable names", async () => {
  const server = await createServer({ root: fileURLToPath(new URL("../app", import.meta.url)), configFile: false, appType: "custom", optimizeDeps: { noDiscovery: true, include: [] }, server: { middlewareMode: true, hmr: false }, plugins: [react()] });
  try {
    const { AgentList } = await server.ssrLoadModule("/src/features/settings/AgentSetup.tsx");
    const agent = { id: "claude", name: "Claude Code", installed: true, loggedIn: true, installHint: "", canInstall: true, installUrl: "" };
    const render = (apiKeyEnv, showApiKeyWarning = true, id = "claude", billingMode = undefined) => renderToStaticMarkup(createElement(AgentList, {
      state: { agents: [{ ...agent, id, apiKeyEnv }], busy: {}, notes: {}, checking: false }, showApiKeyWarning, billingMode,
    }));
    const html = render(["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"]);
    assert.match(html, /role="note"/);
    assert.match(html, /ANTHROPIC_API_KEY, ANTHROPIC_AUTH_TOKEN/);
    assert.match(html, /Claude의 비대화형 실행\(-p\)에서 키를 사용하면 구독 대신 API로 과금됩니다/);
    assert.match(render(["ANTHROPIC_AUTH_TOKEN"]), /CLI의 인증 방식과 과금이 달라질 수 있습니다/);
    assert.match(render(["CODEX_API_KEY"], true, "codex"), /CLI의 인증 방식과 과금이 달라질 수 있습니다/);
    for (const value of [[], undefined]) assert.doesNotMatch(render(value), /인증 환경변수/);
    assert.doesNotMatch(render(["ANTHROPIC_API_KEY"], false), /인증 환경변수/);
    assert.doesNotMatch(html, /구독만 모드/);
    assert.doesNotMatch(render(["ANTHROPIC_API_KEY"], true, "claude", "warn"), /구독만 모드/);
    const refused = render(["CODEX_API_KEY"], true, "codex", "subscription");
    assert.match(refused, /구독만 모드라 이 변수가 있는 동안 이 CLI의 작업은 시작하지 않습니다/);
    assert.doesNotMatch(refused, /사용 전 인증 설정을 확인해 주세요/);
  } finally {
    await server.close();
  }
});
