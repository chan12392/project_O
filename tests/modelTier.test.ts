import { test } from "node:test";
import assert from "node:assert/strict";
import { modelTier } from "../app/src/features/graph/modelTier.ts";

test("luna and haiku ids are the low tier", () => {
  for (const m of ["gpt-6.1-luna", "luna", "claude-haiku-5-5", "haiku"]) {
    assert.equal(modelTier(m), "낮음", m);
  }
});

test("sol, astra, opus and fable ids are the high tier", () => {
  for (const m of ["gpt-6.1-sol", "gpt-6.1-astra", "opus", "claude-opus-5-5", "fable", "claude-fable-5-1"]) {
    assert.equal(modelTier(m), "높음", m);
  }
});

test("terra, sonnet and unknown ids are the default tier", () => {
  for (const m of ["gpt-6.1-terra", "claude-sonnet-5-5", "sonnet", "some-model-9"]) {
    assert.equal(modelTier(m), "보통", m);
  }
});

test("whole words only: 'absolute' does not read as 'sol'", () => {
  assert.equal(modelTier("absolute-model"), "보통");
});
