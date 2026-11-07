import { test } from "node:test";
import assert from "node:assert/strict";
import { countText, deepStampText, escapeHtml, finite } from "../app/src/features/graph/graphText.ts";

test("escapeHtml escapes the five HTML-significant characters", () => {
  assert.equal(escapeHtml("plain label"), "plain label");
  assert.equal(escapeHtml(`<img src=x onerror="alert('1')">`), "&lt;img src=x onerror=&quot;alert(&#39;1&#39;)&quot;&gt;");
  assert.equal(escapeHtml("a & b < c > d"), "a &amp; b &lt; c &gt; d");
  // Escaping is idempotent only for plain input; already-escaped text re-escapes &, as it should.
  assert.equal(escapeHtml("&lt;tag&gt;"), "&amp;lt;tag&amp;gt;");
});

test("deepStampText joins provider, model and a parsed date; bad dates drop the date", () => {
  const stamp = deepStampText({ at: "2026-01-05T09:30:00.000Z", provider: "codex", model: "gpt-6.1-terra" });
  assert.ok(stamp.startsWith("Deep: codex gpt-6.1-terra · "), stamp);
  assert.equal(deepStampText({ at: "not-a-date", provider: "claude", model: "sonnet" }), "Deep: claude sonnet");
  assert.equal(deepStampText({ at: "", provider: "codex", model: "" }), "Deep: codex ");
});

test("countText shows finite numbers and '—' for NaN or infinite", () => {
  assert.equal(countText(0), "0");
  assert.equal(countText(42), "42");
  assert.equal(countText(NaN), "—");
  assert.equal(countText(Infinity), "—");
});

test("finite narrows to real finite numbers", () => {
  assert.equal(finite(3), true);
  assert.equal(finite(0), true);
  assert.equal(finite(null), false);
  assert.equal(finite(undefined), false);
  assert.equal(finite(NaN), false);
  assert.equal(finite(Infinity), false);
});
