import type { CardView, RunView } from "./snapshot.ts";

/** Keep the original team visible while each card continues in its latest run. */
export function agentPanes(runs: readonly RunView[], selected: string): { run: RunView; card: CardView }[] {
  const byId = new Map(runs.map((run) => [run.runId, run]));
  let root = byId.get(selected);
  if (!root) return [];
  const visited = new Set<string>();
  while (root.followup && !visited.has(root.runId)) {
    visited.add(root.runId);
    const previous = byId.get(root.followup.sourceRunId);
    if (!previous || previous.project !== root.project) break;
    root = previous;
  }
  const original = root;
  const belongs = (run: RunView, cardId: string) => {
    const seen = new Set<string>();
    let next: RunView | undefined = run;
    while (next?.followup && next.followup.cardId === cardId && !seen.has(next.runId)) {
      seen.add(next.runId);
      next = byId.get(next.followup.sourceRunId);
      if (next?.project !== original.project) return false;
      if (next?.runId === original.runId) return true;
    }
    return false;
  };
  return original.cards.map((card) => {
    const continuations = runs.filter((run) => belongs(run, card.id) && run.cards.some((item) => item.id === card.id && item.provider === card.provider)).sort((a, b) => a.startedAt.localeCompare(b.startedAt) || a.runId.localeCompare(b.runId));
    const parts = [card, ...continuations.flatMap((run) => run.cards.filter((item) => item.id === card.id && item.provider === card.provider))];
    const latest = continuations.at(-1) ?? original;
    return { run: latest, card: { ...parts[parts.length - 1], log: parts.flatMap((part) => part.log) } };
  });
}
