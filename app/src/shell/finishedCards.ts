import { isSessionRun } from "../../../src/core/attention.ts";
import type { CardView, RunView } from "../../../src/core/snapshot.ts";

/**
 * The cards of O runs that finished since the last look, from every run the app reads, not only
 * the one on screen. Claude Code sessions count only while `watched`. `seen` holds the
 * `runId/cardId` of finished cards and `known` the runs looked at before; both are updated.
 * A run seen for the first time stays quiet: its finished cards are history, not news.
 */
export function newlyFinished(runs: readonly RunView[], seen: Set<string>, known: Set<string>, watched?: string): { run: RunView; card: CardView }[] {
  const news: { run: RunView; card: CardView }[] = [];
  for (const run of runs) {
    if (isSessionRun(run) && run.runId !== watched) continue;
    const first = !known.has(run.runId);
    known.add(run.runId);
    for (const card of run.cards) {
      const key = `${run.runId}/${card.id}`;
      if (!card.endedAt || seen.has(key)) continue;
      seen.add(key);
      if (!first) news.push({ run, card });
    }
  }
  return news;
}
