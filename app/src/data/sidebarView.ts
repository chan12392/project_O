import { useCallback, useEffect, useState } from "react";

import { chipSummary, folderKey, isRecent } from "../../../src/core/projects.ts";
import { failedBeforeCards, type RunView } from "../../../src/core/snapshot.ts";
import { readStored, writeStored } from "../lib/storage.ts";
import { isDirecting, type Project } from "./projects.ts";

/** How the sidebar splits the project list: flat, the user's own groups, or what each project is doing. */
export type GroupBy = "none" | "group" | "status";
export type SortBy = "recent" | "name" | "manual";
/** Run rows under a project before the rest fold into "이전 실행 N개". */
export type RunLimit = 0 | 3 | 5 | "all";

/** The default number of run rows under a project. */
export const SHOWN_RUNS = 3;

export const GROUP_BYS: readonly GroupBy[] = ["none", "group", "status"];
export const SORT_BYS: readonly SortBy[] = ["recent", "name", "manual"];
export const RUN_LIMITS: readonly RunLimit[] = [0, SHOWN_RUNS, 5, "all"];

/** The sidebar's 워크스페이스 옵션, kept in this browser like the user's groups. */
export interface SidebarView {
  groupBy: GroupBy;
  sortBy: SortBy;
  /** The manual order: project paths, the top one first. A project not in it yet sits after the ones that are. */
  order: string[];
  /** Leaves runs an automation started out of the run rows and their counts. */
  hideScheduled: boolean;
  runLimit: RunLimit;
}

/** What the sidebar showed before it had options. */
export const DEFAULT_VIEW: SidebarView = { groupBy: "group", sortBy: "recent", order: [], hideScheduled: false, runLimit: SHOWN_RUNS };

const VIEW_KEY = "project-o-sidebar-view";

/** A saved view, each choice checked on its own: one that is missing or not a known value goes back to its default. */
export function parseView(saved: unknown): SidebarView {
  if (typeof saved !== "object" || saved === null || Array.isArray(saved)) return DEFAULT_VIEW;
  const v = saved as Record<string, unknown>;
  const known = <T,>(values: readonly T[], value: unknown, fallback: T) => (values.includes(value as T) ? (value as T) : fallback);
  return {
    groupBy: known(GROUP_BYS, v.groupBy, DEFAULT_VIEW.groupBy),
    sortBy: known(SORT_BYS, v.sortBy, DEFAULT_VIEW.sortBy),
    order: Array.isArray(v.order) ? v.order.filter((path): path is string => typeof path === "string" && path !== "") : DEFAULT_VIEW.order,
    hideScheduled: typeof v.hideScheduled === "boolean" ? v.hideScheduled : DEFAULT_VIEW.hideScheduled,
    runLimit: known(RUN_LIMITS, v.runLimit, DEFAULT_VIEW.runLimit),
  };
}

type ViewChange = Partial<SidebarView> | ((view: SidebarView) => Partial<SidebarView>);

export function useSidebarView(): [SidebarView, (change: ViewChange) => void] {
  const [view, setView] = useState(() => readStored(VIEW_KEY, DEFAULT_VIEW, parseView));
  useEffect(() => writeStored(VIEW_KEY, view), [view]);
  const change = useCallback((next: ViewChange) => setView((v) => ({ ...v, ...(typeof next === "function" ? next(v) : next) })), []);
  return [view, change];
}

export function isScheduled(run: RunView): boolean {
  return run.scheduled === true;
}

/** Keep each branch's runs and the branch sections in their existing newest-first order. */
export function runsByBranch(runs: readonly RunView[]): { branch: string | undefined; runs: RunView[] }[] {
  const groups = new Map<string | undefined, RunView[]>();
  for (const run of runs) {
    const branch = run.branch || undefined;
    const grouped = groups.get(branch) ?? [];
    grouped.push(run);
    groups.set(branch, grouped);
  }
  return [...groups].map(([branch, runs]) => ({ branch, runs }));
}

/** The projects without the runs an automation started; a project with none of them stays the same object. */
export function withoutScheduled(projects: readonly Project[]): Project[] {
  return projects.map((p) => (p.runs.some(isScheduled) ? { ...p, runs: p.runs.filter((r) => !isScheduled(r)) } : p));
}

/** How many run rows a limit shows before the fold. */
export function shownRuns(limit: RunLimit): number {
  return limit === "all" ? Infinity : limit;
}

/** Newest activity first; quiet projects after, by name (the order the list arrives in). */
const byRecent = (a: Project, b: Project) => (b.lastAt ?? "").localeCompare(a.lastAt ?? "") || a.name.localeCompare(b.name, "ko");

/** Where each path sits in a manual order; a path written twice keeps its first place. */
function places(order: readonly string[]): Map<string, number> {
  const at = new Map<string, number>();
  order.forEach((path, i) => {
    const key = folderKey(path);
    if (!at.has(key)) at.set(key, i);
  });
  return at;
}

/** The projects in the chosen order. Manual: as `order` names them, then the ones it does not know, newest activity first. */
export function sortProjects(projects: readonly Project[], sortBy: SortBy, order: readonly string[] = []): Project[] {
  const sorted = [...projects].sort(byRecent);
  if (sortBy === "name") return sorted.sort((a, b) => a.name.localeCompare(b.name, "ko") || folderKey(a.path).localeCompare(folderKey(b.path)));
  if (sortBy === "manual") {
    const at = places(order);
    const place = (p: Project) => at.get(folderKey(p.path)) ?? order.length;
    return sorted.sort((a, b) => place(a) - place(b));
  }
  return sorted;
}

/**
 * The manual order with every listed project in it: `listed` is the projects as shown now, and
 * the ones the order does not know yet join its end. Paths the list no longer has keep their
 * place (the list may be back later).
 */
export function fullOrder(order: readonly string[], listed: readonly string[]): string[] {
  const seen = new Set<string>();
  return [...order, ...listed].filter((p) => {
    const key = folderKey(p);
    return !seen.has(key) && Boolean(seen.add(key));
  });
}

/** The manual order after `path` is put right before or after `target`; the move starts from what is on screen (`fullOrder`). */
export function placeAt(order: readonly string[], listed: readonly string[], path: string, target: string, side: "before" | "after"): string[] {
  const full = fullOrder(order, listed);
  const moved = full.find((p) => folderKey(p) === folderKey(path));
  if (moved === undefined || folderKey(path) === folderKey(target)) return full;
  const rest = full.filter((p) => p !== moved);
  const to = rest.findIndex((p) => folderKey(p) === folderKey(target));
  if (to < 0) return full;
  rest.splice(side === "after" ? to + 1 : to, 0, moved);
  return rest;
}

export type ProjectStatus = "active" | "attention" | "quiet";

export const STATUS_ORDER: readonly ProjectStatus[] = ["active", "attention", "quiet"];
export const STATUS_WORD: Record<ProjectStatus, string> = { active: "진행 중", attention: "확인 필요", quiet: "조용함" };

/**
 * What a project is doing, for the 상태 grouping. A card that failed, waits for an answer or is
 * held by a usage limit (the run row's chips, without "조용함") makes it "확인 필요" even while a
 * run goes: a limit only ever holds a running card. So does a run that failed before it had
 * cards. Only recent runs count, so a failure from weeks ago does not keep a project there for good.
 */
export function projectStatus(project: Pick<Project, "runs">, now: Date): ProjectStatus {
  const wantsLook = project.runs.some((run) => isRecent(run, now) && (failedBeforeCards(run) || chipSummary(run, now).attention.some((a) => a.why !== "silent")));
  if (wantsLook) return "attention";
  return project.runs.some(isDirecting) ? "active" : "quiet";
}

/** The projects under each status, in STATUS_ORDER and keeping their order; a status with none is left out. */
export function byStatus(projects: readonly Project[], now: Date): { status: ProjectStatus; projects: Project[] }[] {
  const of = new Map(projects.map((p) => [p, projectStatus(p, now)]));
  return STATUS_ORDER.map((status) => ({ status, projects: projects.filter((p) => of.get(p) === status) })).filter((s) => s.projects.length > 0);
}
