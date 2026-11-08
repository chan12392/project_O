import { useEffect, useMemo, useRef, useState } from "react";

import type { Backend } from "../../platform/backend/types.ts";
import { createViewLoader, keyOf, type GraphScope, type ListedProject, type LoaderState } from "./graphLoad.ts";

export type { GraphScope, ListedProject } from "./graphLoad.ts";

/** The project this scope draws, or undefined on the overall scope. */
export const projectOf = (scope: GraphScope) => (scope.scope === "project" ? scope.project : undefined);

/**
 * `o graph view|part` for the scope on screen (ADR-0033). Loads on a scope change and on
 * `refresh` (the toolbar's 새로고침, after 인덱스 만들기, after Deep); the overall scope
 * shows a skeleton immediately and merges each project's part as it lands. An answer that
 * lands after the person picked another scope is dropped, and a refresh only applies
 * while its scope key is still current (#81).
 */
export function useGraphView(api: Backend, scope: GraphScope, projects: ListedProject[]) {
  const [state, setState] = useState<LoaderState>({ loading: true });
  /** What the loader reads at call time, so a captured refresh can't aim at a stale scope. */
  const live = useRef({ scope, projects });
  live.current = { scope, projects };

  const loader = useMemo(
    () =>
      createViewLoader({
        api,
        scope: () => live.current.scope,
        projects: () => live.current.projects,
        emit: setState,
      }),
    [api],
  );

  const key = keyOf(scope);
  // The overall graph is drawn from the project list, which can arrive after the tab opened:
  // a changed list loads it again (the parts already grown stay).
  const listed = scope.scope === "overall" ? projects.filter((p) => !p.missing).map((p) => p.path).join("\n") : "";
  useEffect(() => {
    void loader.refresh();
  }, [loader, key, listed]);

  const current = state.key === key;
  return { view: current ? state.view : undefined, error: current ? state.error : undefined, loading: state.loading || !current, refresh: loader.refresh };
}
