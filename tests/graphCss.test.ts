import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

// The desktop CSP (`style-src 'self'`) blocks the <style> tags 3d-force-graph and its parts
// inject, so their layout rules must ship in the app's own stylesheet: without an absolute
// tooltip, hovering a node pushed the whole graph down.
test("the graph libraries' injected layout rules ship in the app stylesheet", () => {
  const css = readFileSync(new URL("../app/src/styles/graph.css", import.meta.url), "utf8");
  const app = readFileSync(new URL("../app/src/styles/app.css", import.meta.url), "utf8");
  assert.match(app, /@import "\.\/graph\.css";/);
  for (const selector of [".float-tooltip-kap", ".scene-nav-info", ".graph-info-msg"]) {
    const block = new RegExp(`${selector.replace(".", "\\.")}\\s*\\{[^}]*position:\\s*absolute`);
    assert.match(css, block, `${selector} must be absolutely positioned`);
  }
  const conf = JSON.parse(readFileSync(new URL("../app/src-tauri/tauri.conf.json", import.meta.url), "utf8"));
  assert.equal(conf.app.security.csp["style-src"], "'self'", "if the CSP ever allows inline styles, this file may be unnecessary");
});
