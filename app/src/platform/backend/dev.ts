import type { ActionResult, AgentStatus, Backend, BudgetReport, ReviewStarted, TokenReport } from "./types.ts";
import { DESKTOP_UPDATES_ONLY, type About, type UpdateCheck } from "../updates.ts";

async function json<T>(res: Response): Promise<T> {
  if (!res.ok) throw new Error((await res.text()) || `HTTP ${res.status}`);
  return (await res.json()) as T;
}

/** The dev server: the /api routes of app/dev/liveRuns.ts, each answered by `o`. */
export const devBackend: Backend = {
  kind: "dev",
  agents: (clean) => fetch(`/api/agents?clean=${clean ? 1 : 0}`).then((r) => json<AgentStatus[]>(r)),
  install: (id) => fetch(`/api/agents/${id}/install`, { method: "POST" }).then((r) => json<ActionResult>(r)),
  login: (id, clean) => fetch(`/api/agents/${id}/login?clean=${clean ? 1 : 0}`, { method: "POST" }).then((r) => json<ActionResult>(r)),
  openExternal: async (url) => void window.open(url, "_blank", "noopener"),
  about: () => fetch("/api/about").then((r) => json<About>(r)),
  checkUpdate: () => fetch("/api/updates").then((r) => json<UpdateCheck>(r)),
  pendingUpdate: async () => null,
  installUpdate: async () => { throw new Error(DESKTOP_UPDATES_ONLY); },
  cancelUpdate: async () => {},
  review: async (project, pr, clean) => {
    const res = await fetch(`/api/review?${new URLSearchParams({ project, pr, clean: clean ? "1" : "0" })}`, { method: "POST" });
    const body = (await res.json().catch(() => undefined)) as (ReviewStarted & { error?: string }) | undefined;
    if (!res.ok || !body) throw new Error(body?.error ?? `HTTP ${res.status}`);
    return body;
  },
  budget: () => fetch("/api/budget").then((r) => json<BudgetReport>(r)),
  setBudgetPercent: (percent) => fetch(`/api/budget/percent?value=${percent}`, { method: "POST" }).then((r) => json<BudgetReport>(r)),
  tokens: (project) => fetch(`/api/tokens${project ? `?project=${encodeURIComponent(project)}` : ""}`).then((r) => json<TokenReport>(r)),
};
