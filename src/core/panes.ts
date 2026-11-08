/** How many agent panes the work view shows side by side. */
export type PaneCount = 1 | 2 | 4;

export const PANE_COUNTS: readonly PaneCount[] = [1, 2, 4];

/** The layout a team opens with: one pane for a lone agent, otherwise two. */
export function defaultPaneCount(agents: number): PaneCount {
  return agents >= 2 ? 2 : 1;
}

/** The split a wanted count really gets: two panes need two agents, four need three. */
export function fitPaneCount(wanted: PaneCount, agents: number): PaneCount {
  if (agents < 2) return 1;
  return wanted === 4 && agents < 3 ? 2 : wanted;
}

/**
 * The agents shown in panes, in pane order: the ones already open that still exist, then the
 * next unopened agents in team order, never more panes than agents.
 */
export function fitPanes(open: readonly string[], ids: readonly string[], count: PaneCount): string[] {
  const kept = open.filter((id, i) => ids.includes(id) && open.indexOf(id) === i).slice(0, count);
  const filler = ids.filter((id) => !kept.includes(id));
  return [...kept, ...filler].slice(0, Math.min(count, ids.length));
}

/**
 * Clicking a tab: an agent already in a pane only moves the focus there; another one
 * replaces the focused pane's agent.
 */
export function showInPane(open: readonly string[], focus: number, id: string): { open: string[]; focus: number } {
  const at = open.indexOf(id);
  if (at >= 0) return { open: [...open], focus: at };
  if (open.length === 0) return { open: [id], focus: 0 };
  const target = Math.min(Math.max(focus, 0), open.length - 1);
  return { open: open.map((current, i) => (i === target ? id : current)), focus: target };
}

/** A card's unsent follow-up and whether it is on its way. */
export type Composer = { draft: string; sending: boolean };

export const EMPTY_COMPOSER: Composer = { draft: "", sending: false };

/**
 * The team view keeps each card's composer by card ID, outside the panes, so switching a
 * tab or showing fewer panes hides a draft instead of dropping it. The view is keyed by
 * run, so another run's card with the same ID never sees it. Card IDs that collide with
 * Object.prototype ("constructor", "__proto__") are safe: only own properties are read.
 */
export function withComposer(all: Readonly<Record<string, Composer>>, card: string, change: Partial<Composer>): Record<string, Composer> {
  return { ...all, [card]: { ...(Object.hasOwn(all, card) ? all[card] : EMPTY_COMPOSER), ...change } };
}
