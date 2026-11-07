/**
 * A model's usage tier for the Deep warning: how much of the person's subscription one
 * run roughly spends (ADR-0033 — "상위 모델일수록 차감량이 늘어납니다"). Pure: the id is
 * split into name parts, so "gpt-6.1-luna" and "claude-haiku-5-5" both land on their tier.
 */
export type ModelTier = "낮음" | "보통" | "높음";

const LOW = ["luna", "haiku"];
const HIGH = ["sol", "astra", "opus", "fable"];
const MID = ["terra", "sonnet"];

export function modelTier(model: string): ModelTier {
  // Whole words only: "absolute" must not read as "sol".
  const parts = model.toLowerCase().split(/[^a-z0-9]+/);
  const has = (name: string) => parts.includes(name);
  if (LOW.some(has)) return "낮음";
  if (HIGH.some(has)) return "높음";
  // Mid and everything unlisted: the default tier.
  return "보통";
}
