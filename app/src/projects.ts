import { useCallback, useEffect, useRef, useState } from "react";

import type { RunView } from "../../src/core/snapshot.ts";
import { readStored, writeStored } from "./storage.ts";

export interface Project {
  path: string;
  name: string;
  /** Newest first. */
  runs: RunView[];
}

/** Projects as Orca lists repos: one per folder the lead has dispatched in, newest activity first. */
export function projectsOf(runs: RunView[]): Project[] {
  const byPath = new Map<string, Project>();
  for (const run of runs) {
    const project = byPath.get(run.project) ?? { path: run.project, name: run.project.split(/[\\/]/).filter(Boolean).pop() ?? run.project, runs: [] };
    project.runs.push(run);
    byPath.set(run.project, project);
  }
  for (const p of byPath.values()) p.runs.sort((a, b) => b.startedAt.localeCompare(a.startedAt));
  return [...byPath.values()].sort((a, b) => b.runs[0].startedAt.localeCompare(a.runs[0].startedAt));
}

interface Group {
  id: string;
  name: string;
  collapsed: boolean;
}

interface Groups {
  groups: Group[];
  /** Project path → group id. Projects not listed sit outside any group. */
  member: Record<string, string>;
}

const GROUPS_KEY = "project-o-groups";

const loadGroups = () =>
  readStored<Groups>(GROUPS_KEY, { groups: [], member: {} }, (saved) => {
    const g = saved as Groups | null;
    return g && Array.isArray(g.groups) && typeof g.member === "object" ? g : undefined;
  });

/** The user's project groups, kept in this browser like Orca keeps its sidebar layout. */
export function useProjectGroups() {
  const [state, setState] = useState(loadGroups);
  useEffect(() => writeStored(GROUPS_KEY, state), [state]);

  const addGroup = useCallback((name: string, project?: string) => {
    const id = `g${Date.now().toString(36)}`;
    setState((s) => ({ groups: [...s.groups, { id, name, collapsed: false }], member: project ? { ...s.member, [project]: id } : s.member }));
  }, []);
  const move = useCallback((project: string, group: string | undefined) => {
    setState((s) => {
      const member = { ...s.member };
      if (group) member[project] = group;
      else delete member[project];
      return { ...s, member };
    });
  }, []);
  const toggle = useCallback((id: string) => setState((s) => ({ ...s, groups: s.groups.map((g) => (g.id === id ? { ...g, collapsed: !g.collapsed } : g)) })), []);
  const rename = useCallback((id: string, name: string) => setState((s) => ({ ...s, groups: s.groups.map((g) => (g.id === id ? { ...g, name } : g)) })), []);
  const remove = useCallback(
    (id: string) =>
      setState((s) => ({ groups: s.groups.filter((g) => g.id !== id), member: Object.fromEntries(Object.entries(s.member).filter(([, g]) => g !== id)) })),
    [],
  );
  return { ...state, addGroup, move, toggle, rename, remove };
}

/** A project's lead: working while a run is live, otherwise resting until called. */
export type LeadState = "working" | "resting" | "loading" | "ready";

/** How long the preview takes to "load" a session; the desktop build reads the session file instead. */
const LOAD_MS = 900;
/** An idle lead goes back to rest after this long without being called. */
const REST_MS = 10 * 60_000;

export function useLeads(projects: Project[]) {
  const [awake, setAwake] = useState<Record<string, "loading" | "ready">>({});
  const timers = useRef(new Map<string, number[]>());
  useEffect(() => () => timers.current.forEach((ids) => ids.forEach((id) => window.clearTimeout(id))), []);

  const live = projects.filter((p) => p.runs.some((r) => r.status === "running")).map((p) => p.path);

  /** Restarts a lead's timers: an optional load into ready, then rest after REST_MS. */
  const schedule = useCallback((path: string, loading: boolean) => {
    timers.current.get(path)?.forEach((id) => window.clearTimeout(id));
    const ids = [window.setTimeout(() => setAwake(({ [path]: _, ...rest }) => rest), REST_MS)];
    if (loading) ids.push(window.setTimeout(() => setAwake((a) => ({ ...a, [path]: "ready" })), LOAD_MS));
    timers.current.set(path, ids);
  }, []);

  const wake = useCallback(
    (path: string) => {
      setAwake((a) => (a[path] ? a : { ...a, [path]: "loading" }));
      schedule(path, true);
    },
    [schedule],
  );

  // A lead directing a run has its session open, so it needs no loading; it stays
  // awake for a while after the run ends, even when the rest timer ran out mid-run.
  const liveKey = live.join("\n");
  const wasLive = useRef<string[]>([]);
  useEffect(() => {
    for (const path of new Set([...live, ...wasLive.current])) {
      setAwake((a) => (a[path] === "ready" ? a : { ...a, [path]: "ready" }));
      schedule(path, false);
    }
    wasLive.current = live;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [liveKey, schedule]);

  const stateOf = (path: string): LeadState => (live.includes(path) ? "working" : (awake[path] ?? "resting"));
  return { stateOf, wake };
}

export const LEAD_WORD: Record<LeadState, string> = { working: "지휘 중", resting: "쉬는 중", loading: "불러오는 중", ready: "대기 중" };
