/** What the pop-out panel opens on. */
export interface PopoutTarget {
  view: string;
  project?: string;
  run?: string;
  card?: string;
  /** The project's open lead tab; the pop-out shows the same one (ADR-0034). */
  thread?: string;
}

/**
 * The pop-out panel's address: this page with only the panel's own parameters, plus the
 * runs source the main window is showing, so the pop-out shows the same runs (a main window
 * that fell back to the demo must not open a pop-out that picks this machine's live runs).
 * `source` is left out while the main window has not decided yet.
 */
export function popoutUrl(href: string, target: PopoutTarget, source?: "live" | "demo"): URL {
  const url = new URL(href);
  url.search = "";
  url.searchParams.set("popout", "panel");
  if (source) url.searchParams.set("source", source);
  url.searchParams.set("view", target.view);
  if (target.project) url.searchParams.set("project", target.project);
  if (target.run) url.searchParams.set("run", target.run);
  if (target.card) url.searchParams.set("card", target.card);
  if (target.thread) url.searchParams.set("thread", target.thread);
  return url;
}
