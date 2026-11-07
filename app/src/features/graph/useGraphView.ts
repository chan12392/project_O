import { useCallback, useEffect, useRef, useState } from "react";

import type { Backend, GraphView } from "../../platform/backend/types.ts";

/** Which graph is on screen: every project ("overall") or one folder. */
export type GraphScope = { scope: "overall" } | { scope: "project"; project: string };

/** The project this scope draws, or undefined on the overall scope. */
export const projectOf = (scope: GraphScope) => (scope.scope === "project" ? scope.project : undefined);

const keyOf = (scope: GraphScope) => (scope.scope === "overall" ? "overall" : scope.project);

interface State {
  /** The scope this answer belongs to; a stale one never shows. */
  key?: string;
  view?: GraphView;
  error?: string;
  loading: boolean;
}

/**
 * `o graph view` for the scope on screen (ADR-0033). Loads on a scope change and on
 * `refresh` (the toolbar's 새로고침, after 인덱스 만들기, after Deep); an answer that
 * lands after the person picked another scope is dropped.
 */
export function useGraphView(api: Backend, scope: GraphScope) {
  const [state, setState] = useState<State>({ loading: true });
  const seq = useRef(0);
  const key = keyOf(scope);

  const refresh = useCallback(async () => {
    const mine = ++seq.current;
    const project = projectOf(scope);
    setState((s) => ({ ...s, loading: true }));
    try {
      const view = await api.graphView(project);
      if (seq.current === mine) setState({ key, view, loading: false });
    } catch (e) {
      if (seq.current === mine) setState((s) => ({ key, view: s.key === key ? s.view : undefined, error: e instanceof Error ? e.message : String(e), loading: false }));
    }
  }, [api, scope, key]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const current = state.key === key;
  return { view: current ? state.view : undefined, error: current ? state.error : undefined, loading: state.loading || !current, refresh };
}
