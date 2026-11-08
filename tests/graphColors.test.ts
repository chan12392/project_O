import assert from "node:assert/strict";
import { test } from "node:test";

import type { GraphNode } from "../app/src/platform/backend/types.ts";
import { areaColor, baseColor, extOf, languageColor, legendItems, topAreaOf } from "../app/src/features/graph/graphColors.ts";

const PAL = { root: "ROOT", project: "PROJECT", area: "AREA", file: "FILE" };

const file = (path: string, extra?: Partial<GraphNode>): GraphNode => ({ id: `f:${path}`, label: path.split("/").pop() ?? path, kind: "file", path, size: 1, ...extra });

test("extOf reads the extension from the path, then the label; dotfiles and bare names have none", () => {
  assert.equal(extOf(file("src/a.ts")), "ts");
  assert.equal(extOf(file("src/core/app.test.TSX")), "tsx");
  assert.equal(extOf({ label: "index.md" }), "md");
  assert.equal(extOf(file("src/README")), "");
  assert.equal(extOf(file(".gitignore")), "");
});

test("languageColor gives each known group a color per theme; unknown extensions get none", () => {
  for (const ext of ["ts", "tsx", "js", "jsx", "rs", "md", "json", "yaml", "toml", "css", "scss", "py", "go", "sh", "ps1", "html"]) {
    assert.ok(languageColor(ext, "dark"), ext);
    assert.ok(languageColor(ext, "light"), ext);
  }
  assert.equal(languageColor("ts", "dark"), languageColor("tsx", "dark"), "same group, same color");
  assert.notEqual(languageColor("ts", "dark"), languageColor("rs", "dark"));
  assert.notEqual(languageColor("ts", "dark"), languageColor("ts", "light"), "dark and light differ for contrast");
  assert.equal(languageColor("svelte", "dark"), undefined);
  assert.equal(languageColor("", "dark"), undefined);
});

test("topAreaOf takes the first path segment, in both scopes; id parsing is the fallback", () => {
  assert.equal(topAreaOf(file("src/core/a.ts")), "src");
  assert.equal(topAreaOf({ id: "d:src/core", path: "src/core" }), "src");
  assert.equal(topAreaOf({ id: "a:C:/p|crates/o-core", path: "crates/o-core" }), "crates");
  // Overall-scope area ids still parse when path is missing.
  assert.equal(topAreaOf({ id: "a:C:/p|app" }), "app");
  assert.equal(topAreaOf({ id: "f:x" }), "");
});

test("areaColor is a stable categorical hue: same folder, same color, in the fixed ramp", () => {
  assert.equal(areaColor("src", "dark"), areaColor("src", "dark"));
  assert.notEqual(areaColor("src", "dark"), areaColor("src", "light"), "per-theme ramps differ");
  assert.equal(areaColor("", "dark"), undefined);
});

test("baseColor paints files by language or top area; kinds without a category keep the kind tone", () => {
  assert.equal(baseColor(file("a.ts"), "language", "dark", PAL), languageColor("ts", "dark"));
  assert.equal(baseColor(file("a.unknownext"), "language", "dark", PAL), "FILE");
  assert.equal(baseColor(file("src/a.ts"), "area", "dark", PAL), areaColor("src", "dark"));
  assert.equal(baseColor({ id: "d:app", label: "app", kind: "area", path: "app" }, "area", "dark", PAL), areaColor("app", "dark"));
  // Areas keep their hue under 언어별 too: the overall graph has no files to color.
  assert.equal(baseColor({ id: "d:app", label: "app", kind: "area", path: "app" }, "language", "dark", PAL), areaColor("app", "dark"));
  // Root and project stay neutral under both rules.
  for (const kind of ["root", "project"] as const) {
    assert.equal(baseColor({ id: "x", label: "x", kind }, "language", "dark", PAL), PAL[kind]);
    assert.equal(baseColor({ id: "x", label: "x", kind }, "area", "dark", PAL), PAL[kind]);
  }
});

test("legendItems lists only the categories on screen, most used first", () => {
  const view = {
    nodes: [
      file("a.ts"),
      file("b.ts"),
      file("c.ts"),
      file("d.rs"),
      file("e.unknownext"),
      file("src/README"),
      { id: "o:root", label: "전체", kind: "root" as const, size: 1 },
      { id: "d:src", label: "src", kind: "area" as const, path: "src", size: 1 },
    ],
  };
  const items = legendItems(view, "language", "dark", "NEUTRAL");
  assert.equal(items[0].label, ".ts");
  assert.equal(items[0].color, languageColor("ts", "dark"));
  const byLabel = new Map(items.map((i) => [i.label, i.color]));
  assert.equal(byLabel.get(".rs"), languageColor("rs", "dark"));
  // An extension outside the known groups still gets a swatch — in the neutral tone.
  assert.equal(byLabel.get(".unknownext"), "NEUTRAL");
  assert.equal(byLabel.get("기타"), "NEUTRAL");
  assert.equal(items.length, 4);
});

test("area legend names the top-level folders of the visible nodes and caps at 8", () => {
  const many = Array.from({ length: 10 }, (_, i) => file(`dir${i}/f.ts`));
  const items = legendItems({ nodes: many }, "area", "light", "NEUTRAL");
  assert.equal(items.length, 8);
  assert.equal(items[0].color, areaColor(items[0].label, "light"));

  const items2 = legendItems({ nodes: [file("src/a.ts"), file("src/b.rs"), file("app/c.ts")] }, "area", "dark", "NEUTRAL");
  assert.equal(items2[0].label, "src");
  assert.deepEqual(new Set(items2.map((i) => i.label)), new Set(["src", "app"]));
});
