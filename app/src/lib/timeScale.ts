/** Quiet stretches longer than this fold into a thin gap, so minute-long cards hours apart stay readable. */
const FOLD_MS = 10 * 60_000;
/** Percent of the axis each folded gap takes. */
const GAP_SHARE = 3;

export interface Stretch {
  from: number;
  to: number;
  /** Percent from the axis start. */
  left: number;
  /** Percent of the axis. */
  width: number;
}

/**
 * A time axis that skips idle gaps: busy stretches share the width by length,
 * each gap gets a sliver. `at` maps an epoch-ms time to a percent of the axis.
 * Pure (no React) so it can be tested on its own; needs at least one span.
 */
export function timeScale(spans: readonly (readonly [number, number])[]): { at: (t: number) => number; stretches: Stretch[] } {
  const sorted = [...spans].sort((a, b) => a[0] - b[0]);
  const merged: [number, number][] = [];
  for (const [s, e] of sorted) {
    const last = merged.at(-1);
    if (last && s - last[1] <= FOLD_MS) last[1] = Math.max(last[1], e);
    else merged.push([s, Math.max(e, s + 1000)]);
  }
  const room = 100 - GAP_SHARE * (merged.length - 1);
  const total = merged.reduce((sum, [s, e]) => sum + (e - s), 0);
  let left = 0;
  const stretches = merged.map(([from, to]) => {
    const width = (room * (to - from)) / total;
    const stretch = { from, to, left, width };
    left += width + GAP_SHARE;
    return stretch;
  });
  const at = (t: number) => {
    const s = stretches.find((x) => t <= x.to) ?? stretches[stretches.length - 1];
    return s.left + ((Math.min(Math.max(t, s.from), s.to) - s.from) / (s.to - s.from)) * s.width;
  };
  return { at, stretches };
}
