import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { test } from "node:test";
import { pathToFileURL } from "node:url";

import { leadItems, type LeadEvent } from "../src/core/lead.ts";
import { CHAT_MARKDOWN, safeUrl } from "../app/src/features/chat/markdown.ts";
import { imagePaths } from "../app/src/features/chat/imagePaths.ts";

test("finds local image paths in messages and keeps each path once", () => {
  const text = "이미지: `C:\\work\\my file.png`\n![미리보기](C:\\work\\my file.png)\n다른 파일 assets/output.webp";
  const paths = imagePaths(text);
  assert.equal(paths.filter((p) => p === "C:\\work\\my file.png").length, 1);
  assert.ok(paths.includes("assets/output.webp"));
  assert.ok(!imagePaths("문서: notes.txt").length);
  assert.ok(!imagePaths("https://example.com/image.png").length);
});

// React and react-markdown live in the app's packages; load them from there.
const fromApp = createRequire(new URL("../app/package.json", import.meta.url));
const load = (id: string) => import(pathToFileURL(fromApp.resolve(id)).href);
const [{ createElement }, { renderToStaticMarkup }, { default: ReactMarkdown }] = await Promise.all([load("react"), load("react-dom/server"), load("react-markdown")]);

/** What a chat bubble's Markdown becomes, with the app's parser setup. */
const html = (text: string): string => renderToStaticMarkup(createElement(ReactMarkdown, CHAT_MARKDOWN, text));

// The lead reply from the owner's screenshot, plus the rest of GFM and hostile input.
const SAMPLE = [
  "정리했습니다.",
  "",
  "## 진행 상황 요약",
  "",
  "### ✅ 끝난 것",
  "",
  "| 영역 | 내용 | 근거 |",
  "|---|---|---|",
  "| 기술 스택 (D1·D2) | Tauri 2 + Rust 코어 + React 화면으로 정했습니다. **B 승인 없이 A가 결정했고, B의 사후 확인이 필요합니다** | `docs/decisions/0001-stack-tauri-rust-core.md` |",
  "| Rust 이관 | 끝 | `crates/` |",
  "",
  "- 하나",
  "  - 둘",
  "1. 첫째",
  "",
  "- [x] 끝남",
  "- [ ] 남음",
  "",
  "```ts",
  "const a = 1;",
  "```",
  "",
  "> 인용",
  "",
  "---",
  "",
  "*기울임* ~~취소~~ [문서](https://example.com/docs) [나쁜 링크](javascript:alert(1))",
  "",
  "<script>alert(1)</script>",
  "",
  '<img src="x" onerror="alert(1)">',
].join("\n");

test("the lead keeps a reply's line breaks on its way to the screen", () => {
  const text = SAMPLE.split("\n").slice(6, 10).join("\n");
  const event: LeadEvent = { t: "lead.reply", at: "2026-11-02T09:20:05.851Z", provider: "claude", text };
  const [reply] = leadItems([event]);
  assert.equal(reply.kind === "reply" && reply.text, text);
});

test("chat Markdown draws GitHub-flavoured blocks", () => {
  const out = html(SAMPLE);
  assert.match(out, /<h2>진행 상황 요약<\/h2>/);
  assert.match(out, /<h3>✅ 끝난 것<\/h3>/);
  // Each table row is its own row, not one paragraph of pipes.
  assert.match(out, /<table>.*<thead><tr><th>영역<\/th><th>내용<\/th><th>근거<\/th><\/tr><\/thead>/s);
  assert.equal(out.match(/<tr>/g)?.length, 3);
  assert.match(out, /<td><code>docs\/decisions\/0001-stack-tauri-rust-core\.md<\/code><\/td>/);
  assert.match(out, /<strong>B 승인 없이/);
  assert.match(out, /<ul>\s*<li>하나\s*<ul>\s*<li>둘<\/li>/);
  assert.match(out, /<ol>\s*<li>첫째<\/li>/);
  assert.equal(out.match(/type="checkbox"/g)?.length, 2);
  assert.match(out, /<pre><code class="language-ts">const a = 1;\n<\/code><\/pre>/);
  assert.match(out, /<blockquote>\s*<p>인용<\/p>\s*<\/blockquote>/);
  assert.match(out, /<hr\/>/);
  assert.match(out, /<em>기울임<\/em> <del>취소<\/del>/);
  assert.match(out, /<a href="https:\/\/example\.com\/docs">문서<\/a>/);
});

test("chat Markdown never emits raw HTML or a script link", () => {
  const out = html(SAMPLE);
  assert.doesNotMatch(out, /<script|<img|<iframe/i);
  // Shown as the text it is, with no element carrying an event handler.
  assert.doesNotMatch(out, /<[a-z][^>]*\son\w+=/i);
  assert.match(out, /&lt;img src=&quot;x&quot; onerror=&quot;alert\(1\)&quot;&gt;/);
  assert.match(out, /<p>&lt;script&gt;alert\(1\)&lt;\/script&gt;<\/p>/);
  // The javascript: link is text, not a link.
  assert.match(out, /<a>나쁜 링크<\/a>/);
  assert.doesNotMatch(out, /href="(?!https?:)/);
  for (const link of ["[a](JavaScript:alert(1))", "[a](java%0ascript:alert(1))", "[a](data:text/html,<b>x</b>)", "[a](vbscript:x)", "[a](file:///C:/Windows)", "<javascript:alert(1)>"]) {
    assert.doesNotMatch(html(link), /href=/, link);
  }
});

test("images are a link with their alt text, never fetched", () => {
  const out = html("![구성도](https://example.com/a.png) ![추적](javascript:alert(1)) [![배지](https://example.com/b.svg)](https://example.com)");
  assert.doesNotMatch(out, /<img/);
  assert.match(out, /<a href="https:\/\/example\.com\/a\.png" data-image="">구성도<\/a>/);
  assert.match(out, /<a data-image="">추적<\/a>/);
  assert.match(out, /<a href="https:\/\/example\.com\/">배지<\/a>/);
});

test("line breaks inside a paragraph stay, and <br> in a table cell is a break", () => {
  assert.match(html("첫 줄\n둘째 줄"), /<p>첫 줄<br\/>\n?둘째 줄<\/p>/);
  assert.match(html("| a |\n|---|\n| 위<br>아래 |"), /<td>위<br\/>아래<\/td>/);
});

test("the lead's plan block is left to the plan card", () => {
  const out = html('계획입니다.\n\n```o-plan\n{"title":"x"}\n```\n\n끝.');
  assert.doesNotMatch(out, /<pre|o-plan|title/);
  assert.match(out, /<p>계획입니다\.<\/p>\s*<p>끝\.<\/p>/);
});

test("safeUrl opens only plain http(s) pages", () => {
  assert.equal(safeUrl("https://github.com/o/p/pull/1"), "https://github.com/o/p/pull/1");
  assert.equal(safeUrl(" http://localhost:1430/x "), "http://localhost:1430/x");
  for (const url of ["javascript:alert(1)", " JAVASCRIPT:alert(1)", "data:text/html,x", "file:///C:/x", "docs/a.md", "#top", "//evil.example", "https://trusted.com@evil.example/", "mailto:a@b.c"]) {
    assert.equal(safeUrl(url), undefined, url);
  }
});
