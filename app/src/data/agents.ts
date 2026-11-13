import { useCallback, useSyncExternalStore } from "react";

import { errorText } from "../lib/format.ts";
import type { AgentStatus, Backend } from "../platform/backend/index.ts";

/** One profile's agent check, as every screen sees it. */
export interface AgentCheck {
  /** The newest list: this launch's check, else the one the last launch saw (`remembered`). */
  agents?: AgentStatus[];
  /** Why the newest check failed; the list above stays. */
  error?: string;
  checking: boolean;
  /** `agents` comes from an earlier launch and is being checked again. */
  remembered: boolean;
}

interface Storage {
  get: (key: string) => string | null;
  set: (key: string, value: string) => void;
}

const storageKey = (clean: boolean) => `project-o.agents.${clean ? "clean" : "normal"}`;

/**
 * The agent CLIs' state shared by onboarding, Settings → 에이전트 and the composer: `o agents
 * status` takes 8–10 s, so it runs once per profile (clean or not) until someone asks again
 * ("다시 확인", after an install or sign-in), not once per screen. The last answer is kept in
 * this browser's storage, so the next launch can pick a lead at once while it checks again.
 * Only the newest check may land.
 */
export function createAgentStore(load: (clean: boolean) => Promise<AgentStatus[]>, storage?: Storage) {
  const entries = new Map<boolean, { state: AgentCheck; listeners: Set<() => void>; ticket: number; asked: boolean }>();

  const entry = (clean: boolean) => {
    let found = entries.get(clean);
    if (!found) {
      let agents: AgentStatus[] | undefined;
      try {
        const saved = storage?.get(storageKey(clean));
        const parsed: unknown = saved ? JSON.parse(saved) : undefined;
        if (Array.isArray(parsed)) agents = parsed as AgentStatus[];
      } catch {
        // Nothing usable remembered.
      }
      // Checking from the start: the first screen to look starts the check.
      found = { state: { agents, checking: true, remembered: agents !== undefined }, listeners: new Set(), ticket: 0, asked: false };
      entries.set(clean, found);
    }
    return found;
  };

  const set = (clean: boolean, state: AgentCheck) => {
    const e = entry(clean);
    e.state = state;
    for (const listener of e.listeners) listener();
  };

  const check = async (clean: boolean) => {
    const e = entry(clean);
    e.asked = true;
    const ticket = ++e.ticket;
    set(clean, { ...e.state, checking: true });
    try {
      const agents = await load(clean);
      if (ticket !== e.ticket) return;
      set(clean, { agents, checking: false, remembered: false });
      try {
        storage?.set(storageKey(clean), JSON.stringify(agents));
      } catch {
        // Not remembered: the next launch checks first.
      }
    } catch (error) {
      if (ticket === e.ticket) set(clean, { ...e.state, checking: false, error: errorText(error) });
    }
  };

  return {
    get: (clean: boolean) => entry(clean).state,
    /** Listens to a profile's check, starting it the first time anyone looks. */
    subscribe: (clean: boolean, listener: () => void) => {
      const e = entry(clean);
      e.listeners.add(listener);
      if (!e.asked) void check(clean);
      return () => void e.listeners.delete(listener);
    },
    check,
  };
}

const browserStorage: Storage = {
  get: (key) => {
    try {
      return window.localStorage.getItem(key);
    } catch {
      return null;
    }
  },
  set: (key, value) => {
    try {
      window.localStorage.setItem(key, value);
    } catch {
      // Private window or blocked storage: nothing is remembered.
    }
  },
};

const stores = new WeakMap<Backend, ReturnType<typeof createAgentStore>>();

/** The shared agent check of this backend and profile, and a way to run it again. */
export function useAgentCheck(api: Backend, clean: boolean) {
  let store = stores.get(api);
  if (!store) {
    store = createAgentStore((c) => api.agents(c), browserStorage);
    stores.set(api, store);
  }
  const shared = store;
  const subscribe = useCallback((listener: () => void) => shared.subscribe(clean, listener), [shared, clean]);
  const state = useSyncExternalStore(subscribe, () => shared.get(clean));
  const recheck = useCallback(() => shared.check(clean), [shared, clean]);
  return { ...state, recheck };
}
