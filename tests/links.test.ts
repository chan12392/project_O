import assert from "node:assert/strict";
import { test } from "node:test";

import { LINK_BLOCKED, tauriBackend } from "../app/src/platform/backend/tauri.ts";

test("a link the desktop app refuses says why on screen, and the caller still sees the failure", async () => {
  const page = new EventTarget();
  (globalThis as { window?: unknown }).window = page;
  const shown: { url: string; message: string }[] = [];
  page.addEventListener(LINK_BLOCKED, (e) => shown.push((e as CustomEvent<{ url: string; message: string }>).detail));

  // The shell's own refusal (o-core agents::open_external).
  const refusing = tauriBackend(async () => Promise.reject("https 주소만 열 수 있습니다"));
  await assert.rejects(refusing.openExternal("http://example.com/old"));
  assert.deepEqual(shown, [{ url: "http://example.com/old", message: "보안을 위해 https 주소만 브라우저로 엽니다. 이 링크는 열지 않았습니다." }]);

  const opened: unknown[] = [];
  await tauriBackend(async (cmd, args) => void opened.push([cmd, args])).openExternal("https://github.com/example/demo/pull/7");
  assert.deepEqual(opened, [["open_external", { url: "https://github.com/example/demo/pull/7" }]]);
  assert.equal(shown.length, 1);
});
