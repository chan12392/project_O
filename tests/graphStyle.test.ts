import { test } from "node:test";
import assert from "node:assert/strict";
import { hasGlow, linkColor, linkParticles, nodeAura, nodeColor, nodeHalo, nodeSize, palette } from "../app/src/features/graph/graphStyle.ts";

test("nodeAura gives every node a faint light with effects on, and glowing nodes a wide bright one", () => {
  assert.deepEqual(nodeAura({ kind: "file" }, false, false), { reach: 0, alpha: 0 });
  assert.deepEqual(nodeAura({ kind: "file", glow: "active" }, false, true), { reach: 0, alpha: 0 });
  const file = nodeAura({ kind: "file" }, true, true);
  const area = nodeAura({ kind: "area" }, true, true);
  const path = nodeAura({ kind: "file", glow: "path" }, true, true);
  const active = nodeAura({ kind: "file", glow: "active" }, true, true);
  assert.ok(file.alpha > 0 && file.alpha <= 0.25, "faint, not a spotlight");
  assert.ok(area.alpha > file.alpha && area.reach > file.reach);
  assert.ok(path.alpha > area.alpha && active.alpha > path.alpha && active.reach > path.reach);
  // While the reveal holds glow back, a changed file carries only the faint light.
  assert.deepEqual(nodeAura({ kind: "file", glow: "active" }, true, false), file);
});

// A palette with distinct names so each slot is recognizable in assertions.
const CSS: Record<string, string> = {
  "--foreground": "ROOT",
  "--chart-2": "PROJECT",
  "--chart-4": "AREA",
  "--muted-foreground": "FILE",
  "--agent-working": "ACTIVE",
  "--status-warning": "PATH",
  "--border": "LINK",
};
const P = palette((name) => CSS[name] ?? "");

test("palette uses the read values and falls back on empty answers", () => {
  assert.equal(P.root, "ROOT");
  assert.equal(P.project, "PROJECT");
  assert.equal(P.area, "AREA");
  assert.equal(P.file, "FILE");
  assert.equal(P.active, "ACTIVE");
  assert.equal(P.path, "PATH");
  assert.equal(P.link, "LINK");
  // linkGlow reads --agent-working, same as active.
  assert.equal(P.linkGlow, "ACTIVE");
  // Mild dimming: non-glowing nodes stay readable instead of near-invisible.
  assert.equal(P.dim, 0.7);

  const d = palette(() => "");
  assert.equal(d.root, "#f0f0f0");
  assert.equal(d.project, "#3b82f6");
  assert.equal(d.area, "#1d4ed8");
  assert.equal(d.file, "#acacac");
  assert.equal(d.active, "#eab308");
  assert.equal(d.path, "#eab308");
  assert.equal(d.link, "#404040");
  assert.equal(d.linkGlow, "#eab308");
  // Whitespace-only answers fall back too.
  assert.equal(palette(() => "   ").root, "#f0f0f0");
});

test("nodeColor paints kind colors, glow only with effects, and fades dimmed non-glow nodes", () => {
  assert.equal(nodeColor({ kind: "root" }, P, false, false), "ROOT");
  assert.equal(nodeColor({ kind: "project" }, P, false, false), "PROJECT");
  assert.equal(nodeColor({ kind: "area" }, P, false, false), "AREA");
  assert.equal(nodeColor({ kind: "file" }, P, false, false), "FILE");

  // Glow overrides the kind color, but only when effects are on.
  assert.equal(nodeColor({ kind: "file", glow: "active" }, P, true, false), "ACTIVE");
  // A path node keeps its own color; its halo and the flow carry the glow.
  assert.equal(nodeColor({ kind: "file", glow: "path" }, P, true, false), "FILE");
  assert.equal(nodeColor({ kind: "file", glow: "path" }, P, true, false, "#58a6ff"), "#58a6ff");
  assert.equal(nodeColor({ kind: "file", glow: "active" }, P, false, false), "FILE");
  assert.equal(nodeColor({ kind: "file", glow: "path" }, P, false, false), "FILE");

  // Dimmed non-glow nodes fade through fade()'s color-mix string form.
  assert.equal(nodeColor({ kind: "project" }, P, true, true), "color-mix(in srgb, PROJECT 70%, transparent)");
  // Dimming does not apply to glowing nodes, and does nothing without effects.
  assert.equal(nodeColor({ kind: "file", glow: "active" }, P, true, true), "ACTIVE");
  assert.equal(nodeColor({ kind: "project" }, P, false, true), "PROJECT");

  // A 색 기준 base color wins over the kind color; glow still overrides it.
  assert.equal(nodeColor({ kind: "file" }, P, false, false, "#123456"), "#123456");
  assert.equal(nodeColor({ kind: "file", glow: "active" }, P, true, false, "#123456"), "ACTIVE");
  assert.equal(nodeColor({ kind: "file" }, P, true, true, "#123456"), "color-mix(in srgb, #123456 70%, transparent)");
});

test("nodeSize orders kinds like depth levels and grows glowing nodes only with effects", () => {
  const kinds = ["root", "project", "area", "file"] as const;
  const sizes = kinds.map((kind) => nodeSize({ kind, size: 4 }, false));
  for (let i = 1; i < sizes.length; i++) assert.ok(sizes[i - 1] > sizes[i], `${kinds[i - 1]} > ${kinds[i]}`);
  // `size` nudges within a level but never crosses into the next kind's band.
  assert.ok(nodeSize({ kind: "file", size: 1000000 }, false) < nodeSize({ kind: "project", size: 1 }, false));

  const plain = nodeSize({ kind: "file", size: 4 }, false);
  assert.ok(nodeSize({ kind: "file", size: 4, glow: "active" }, true) > plain);
  assert.ok(nodeSize({ kind: "file", size: 4, glow: "path" }, true) > plain);
  assert.ok(nodeSize({ kind: "file", size: 4, glow: "active" }, true) > nodeSize({ kind: "file", size: 4, glow: "path" }, true));
  // Effects off ignores glow entirely.
  assert.equal(nodeSize({ kind: "file", size: 4, glow: "active" }, false), plain);
  assert.equal(nodeSize({ kind: "file", size: 4, glow: "path" }, false), plain);
});

test("nodeHalo is positive only for glowing nodes with effects, active over path", () => {
  const active = nodeHalo({ glow: "active" }, true);
  const path = nodeHalo({ glow: "path" }, true);
  assert.ok(active > 0);
  assert.ok(path > 0);
  assert.ok(active > path);
  assert.equal(nodeHalo({}, true), 0);
  assert.equal(nodeHalo({ glow: "active" }, false), 0);
  assert.equal(nodeHalo({ glow: "path" }, false), 0);
});

test("linkColor brightens glowing links only with effects and fades the rest while something glows", () => {
  // Glow only with effects; linkGlow is the --agent-working value.
  assert.equal(linkColor({ glow: true }, P, true, false), "ACTIVE");
  assert.equal(linkColor({ glow: true }, P, false, false), "LINK");
  assert.equal(linkColor({ glow: true }, P, true, true), "ACTIVE");
  // Dimmed fades only non-glow links, only with effects.
  assert.equal(linkColor({}, P, true, true), "color-mix(in srgb, LINK 70%, transparent)");
  assert.equal(linkColor({}, P, true, false), "LINK");
  assert.equal(linkColor({}, P, false, true), "LINK");
});

test("linkParticles needs effects && particles && link.glow; grows with weight, capped at 6", () => {
  assert.equal(linkParticles({ weight: 1, glow: true }, true, true), 3);
  assert.ok(linkParticles({ weight: 3, glow: true }, true, true) > linkParticles({ weight: 1, glow: true }, true, true));
  assert.equal(linkParticles({ weight: 100, glow: true }, true, true), 6);
  assert.equal(linkParticles({ weight: 1, glow: true }, false, true), 0);
  assert.equal(linkParticles({ weight: 1, glow: true }, true, false), 0);
  assert.equal(linkParticles({ weight: 1 }, true, true), 0);
});

test("non-finite backend numbers behave like 0 and never produce NaN", () => {
  assert.equal(nodeSize({ kind: "file", size: NaN }, false), nodeSize({ kind: "file", size: 0 }, false));
  assert.ok(!Number.isNaN(nodeSize({ kind: "file", size: NaN }, false)));
  assert.equal(linkParticles({ weight: NaN, glow: true }, true, true), 2);
  assert.ok(!Number.isNaN(linkParticles({ weight: NaN, glow: true }, true, true)));
});

test("hasGlow is false for a plain view and true once a node or link glows", () => {
  const node = { id: "f:a", label: "a", kind: "file" as const, size: 1 };
  const link = { source: "a", target: "b", kind: "calls" as const, weight: 1 };
  assert.equal(hasGlow({ nodes: [node], links: [link] }), false);
  assert.equal(hasGlow({ nodes: [{ ...node, glow: "active" }], links: [link] }), true);
  assert.equal(hasGlow({ nodes: [node], links: [{ ...link, glow: true }] }), true);
});
