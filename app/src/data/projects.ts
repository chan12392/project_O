import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from "react";

import { isSessionRun } from "../../../src/core/attention.ts";
import { folderKey, ownerOf, threadStartedAt, type ProjectKind, type ProjectList } from "../../../src/core/projects.ts";
import type { RunView } from "../../../src/core/snapshot.ts";
import { readStored, writeStored } from "../lib/storage.ts";
import type { Backend } from "../platform/backend/index.ts";

export interface Project {
  path: string;
  name: string;
  /** Where the list found it (ADR-0009); "history" only when the list could not be read. */
  kind: ProjectKind | "history";
  /** A registered folder that is gone. */
  missing?: boolean;
  /** Newest first. */
  runs: RunView[];
  /** The newest run start or lead thread start, for ordering. */
  lastAt?: string;
}

const nameOf = (path: string) => path.split(/[\\/]/).filter(Boolean).pop() ?? path;

/** Every folder history worked in, newest first: the sidebar's fallback while the list cannot be read. */
export function projectsOf(runs: RunView[]): Project[] {
  const byKey = new Map<string, Project>();
  for (const run of runs) {
    const key = folderKey(run.project);
    const project = byKey.get(key) ?? { path: run.project, name: nameOf(run.project), kind: "history" as const, runs: [] };
    project.runs.push(run);
    byKey.set(key, project);
  }
  return order([...byKey.values()]);
}

/** Newest activity first; quiet projects after, by name. */
function order(projects: Project[]): Project[] {
  for (const p of projects) {
    p.runs.sort((a, b) => b.startedAt.localeCompare(a.startedAt));
    const run = p.runs[0]?.startedAt;
    if (run && (!p.lastAt || run > p.lastAt)) p.lastAt = run;
  }
  return projects.sort((a, b) => (b.lastAt ?? "").localeCompare(a.lastAt ?? "") || a.name.localeCompare(b.name, "ko"));
}

/**
 * The listed projects (root, test group, registered) with their runs and sessions attached:
 * a run belongs to the deepest listed folder holding it, everything else stays unshown.
 * A lead thread's start counts as activity, so a project just started does not sink (U11).
 */
export function projectsFrom(list: ProjectList, runs: RunView[], threads: { project: string; threadId: string }[] = []): Project[] {
  const projects: Project[] = list.projects.map((p) => ({ path: p.path, name: p.name, kind: p.kind, missing: p.missing, runs: [] }));
  for (const run of runs) ownerOf(projects, run.project)?.runs.push(run);
  for (const thread of threads) {
    const owner = ownerOf(projects, thread.project);
    const at = threadStartedAt(thread.threadId);
    if (owner && at && (!owner.lastAt || at > owner.lastAt)) owner.lastAt = at;
  }
  return order(projects);
}

// The project list, shared by the sidebar, the new-request screen and Settings → 프로젝트.

export interface ListState {
  list?: ProjectList;
  /** Why the list could not be read; the sidebar then falls back to every folder in history. */
  error?: string;
  pending: boolean;
}

let listState: ListState = { pending: true };
let listApi: Backend | undefined;
const listeners = new Set<() => void>();

function setList(next: ListState) {
  listState = next;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => void listeners.delete(listener);
}

/** Reads the list again (a folder made in Explorer shows up on the next read). */
export async function refreshProjects(): Promise<void> {
  const api = listApi;
  if (!api) return;
  try {
    const list = await api.projects();
    if (api === listApi) setList({ list, pending: false });
  } catch (e) {
    if (api === listApi) setList({ ...listState, error: e instanceof Error ? e.message : String(e), pending: false });
  }
}

/** The machine the list is read from; App sets it once it knows whether it shows live runs or the demo. */
export function useProjectSource(api: Backend | undefined) {
  useEffect(() => {
    if (!api) return;
    if (api !== listApi) {
      listApi = api;
      setList({ pending: true });
      void refreshProjects();
    }
    const again = () => document.visibilityState === "visible" && void refreshProjects();
    window.addEventListener("focus", again);
    document.addEventListener("visibilitychange", again);
    return () => {
      window.removeEventListener("focus", again);
      document.removeEventListener("visibilitychange", again);
    };
  }, [api]);
}

export function useProjectList(): ListState {
  return useSyncExternalStore(subscribe, () => listState);
}

/** The backend the list is read from, for the forms that change it. */
export function projectsApi(): Backend | undefined {
  return listApi;
}

/** Runs one change and shows the list it answers with; rejects with the machine's reason. */
export async function changeProjects<T extends ProjectList>(change: (api: Backend) => Promise<T>): Promise<T> {
  const api = listApi;
  if (!api) throw new Error("프로젝트 목록을 아직 읽지 못했습니다");
  const list = await change(api);
  setList({ list, pending: false });
  return list;
}

const IMPORT_KEY = "project-o-import-offered";

/** "기록에서 찾은 프로젝트" is offered once: dismissed or used, it stays away. */
export function importOffered(): boolean {
  return readStored(IMPORT_KEY, false, (saved) => (typeof saved === "boolean" ? saved : undefined));
}

export function markImportOffered() {
  writeStored(IMPORT_KEY, true);
}

const TEST_OPEN_KEY = "project-o-test-group-open";

/** The test group starts folded; its state is kept in this browser like the user's groups. */
export function useTestGroupOpen(): [boolean, () => void] {
  const [open, setOpen] = useState(() => readStored(TEST_OPEN_KEY, false, (saved) => (typeof saved === "boolean" ? saved : undefined)));
  useEffect(() => writeStored(TEST_OPEN_KEY, open), [open]);
  return [open, useCallback(() => setOpen((o) => !o), [])];
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

/** Whether an O run of the project is going: a Claude Code session open on this PC is not the lead directing work (B7). */
export function isDirecting(run: RunView): boolean {
  return run.status === "running" && !isSessionRun(run);
}

export function useLeads(projects: Project[]) {
  const [awake, setAwake] = useState<Record<string, "loading" | "ready">>({});
  const timers = useRef(new Map<string, number[]>());
  useEffect(() => () => timers.current.forEach((ids) => ids.forEach((id) => window.clearTimeout(id))), []);

  const live = projects.filter((p) => p.runs.some(isDirecting)).map((p) => p.path);

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
