import { useEffect, useState } from "react";

import type { Backend, TermSession } from "../../platform/backend/types.ts";

/**
 * `o term list` while `enabled` (ADR-0042): the sessions a lead thread or a card
 * may already have, so reopening the app or a tab finds them again. ~2s like the
 * lead's own poll, slower while the window is hidden; `refresh` asks now.
 */
export function useTermSessions(api: Backend | undefined, enabled: boolean): { sessions: TermSession[] | undefined; refresh: () => void } {
  const [sessions, setSessions] = useState<TermSession[]>();
  const [bump, setBump] = useState(0);
  useEffect(() => {
    if (!enabled || !api) {
      setSessions(undefined);
      return;
    }
    let live = true;
    const tick = () => {
      void api.termList().then((list) => {
        if (live) setSessions(list);
      }, () => {});
    };
    let timer = 0;
    const arm = () => {
      window.clearInterval(timer);
      timer = window.setInterval(tick, document.hidden ? 15_000 : 2_000);
    };
    const onVisibility = () => {
      tick();
      arm();
    };
    tick();
    arm();
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      live = false;
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [api, enabled, bump]);
  return { sessions, refresh: () => setBump((n) => n + 1) };
}
