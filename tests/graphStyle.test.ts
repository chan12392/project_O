import { test } from "node:test";
import assert from "node:assert/strict";
import { fade, hasGlow, linkColor, linkParticles, linkWidth, nodeColor, nodeHalo, nodeSize, palette } from "../app/src/features/graph/graphStyle.ts";

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
  assert.equal(P.dim, 0.25);

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
  assert.equal(nodeColor({ kind: "file", glow: "path" }, P, true, false), "PATH");
  assert.equal(nodeColor({ kind: "file", glow: "active" }, P, false, false), "FILE");
  assert.equal(nodeColor({ kind: "file", glow: "path" }, P, false, false), "FILE");

  // Dimmed non-glow nodes fade through fade()'s color-mix string form.
  assert.equal(nodeColor({ kind: "project" }, P, true, true), "color-mix(in srgb, PROJECT 25%, transparent)");
  assert.equal(nodeColor({ kind: "project" }, P, true, true), fade("PROJECT", P));
  // Dimming does not apply to glowing nodes, and does nothing without effects.
  assert.equal(nodeColor({ kind: "file", glow: "active" }, P, true, true), "ACTIVE");
  assert.equal(nodeColor({ kind: "project" }, P, false, true), "PROJECT");
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

test("linkColor/linkWidth brighten glowing links only with effects; width grows with weight and is capped", () => {
  // Glow only with effects; linkGlow is the --agent-working value.
  assert.equal(linkColor({ glow: true }, P, true, false), "ACTIVE");
  assert.equal(linkColor({ glow: true }, P, false, false), "LINK");
  assert.equal(linkColor({ glow: true }, P, true, true), "ACTIVE");
  // Dimmed fades only non-glow links, only with effects.
  assert.equal(linkColor({}, P, true, true), "color-mix(in srgb, LINK 25%, transparent)");
  assert.equal(linkColor({}, P, true, false), "LINK");
  assert.equal(linkColor({}, P, false, true), "LINK");

  assert.ok(linkWidth({ weight: 4, glow: true }, true) > linkWidth({ weight: 4 }, true));
  assert.equal(linkWidth({ weight: 4, glow: true }, false), linkWidth({ weight: 4 }, false));
  // Width grows with weight and the plain width is capped at 3.
  assert.ok(linkWidth({ weight: 4 }, false) > linkWidth({ weight: 1 }, false));
  assert.equal(linkWidth({ weight: 1000000 }, false), 3);
});

test("linkParticles needs effects && particles && link.glow; grows with weight, capped at 6", () => {
  assert.equal(linkParticles({ weight: 1, glow: true }, true, true), 3);
  assert.ok(linkParticles({ weight: 3, glow: true }, true, true) > linkParticles({ weight: 1, glow: true }, true, true));
  assert.equal(linkParticles({ weight: 100, glow: true }, true, true), 6);
  assert.equal(linkParticles({ weight: 1, glow: true }, false, true), 0);
  assert.equal(linkParticles({ weight: 1, glow: true }, true, false), 0);
  assert.equal(linkParticles({ weight: 1 }, true, true), 0);
});

test("hasGlow is false for a plain view and true once a node or link glows", () => {
  const node = { id: "f:a", label: "a", kind: "file" as const, size: 1 };
  const link = { source: "a", target: "b", kind: "calls" as const, weight: 1 };
  assert.equal(hasGlow({ nodes: [node], links: [link] }), false);
  assert.equal(hasGlow({ nodes: [{ ...node, glow: "active" }], links: [link] }), true);
  assert.equal(hasGlow({ nodes: [node], links: [{ ...link, glow: true }] }), true);
});
