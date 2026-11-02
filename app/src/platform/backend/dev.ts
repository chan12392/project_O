import type { ActionResult, AgentStatus, Backend, BudgetReport, LeadAttachment, LeadDispatched, LeadEvent, LeadModel, LeadProvider, LeadShow, ReviewStarted, TokenReport } from "./types.ts";
import { DESKTOP_UPDATES_ONLY, type About, type UpdateCheck } from "../updates.ts";

async function json<T>(res: Response): Promise<T> {
  if (!res.ok) {
    const text = await res.text();
    // The dev server answers errors as { error }; show the reason, not the JSON.
    let message = text;
    try {
      message = (JSON.parse(text) as { error?: string }).error ?? text;
    } catch {
      // Plain text stays as it is.
    }
    throw new Error(message || `HTTP ${res.status}`);
  }
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
  leadShow: (project) => fetch(`/api/lead/show${project ? `?project=${encodeURIComponent(project)}` : ""}`).then((r) => json<LeadShow>(r)),
  leadNew: (project) => fetch(`/api/lead/new?${new URLSearchParams({ project })}`, { method: "POST" }).then((r) => json<{ threadId: string }>(r)),
  leadAttach: (project, file) =>
    fetch(`/api/lead/attach?${new URLSearchParams({ project, name: file.name })}`, { method: "POST", body: file }).then((r) => json<LeadAttachment>(r)),
  leadSend: (request) =>
    fetch(`/api/lead/send?${new URLSearchParams({ project: request.project })}`, { method: "POST", body: JSON.stringify(request) }).then((r) =>
      json<{ threadId: string | null; events: LeadEvent[] }>(r),
    ),
  leadDispatch: (planId, clean) =>
    fetch(`/api/lead/dispatch?${new URLSearchParams({ plan: planId, clean: clean ? "1" : "0" })}`, { method: "POST" }).then((r) => json<LeadDispatched>(r)),
  leadStop: (project) => fetch(`/api/lead/stop${project ? `?project=${encodeURIComponent(project)}` : ""}`, { method: "POST" }).then((r) => json<{ stopped: boolean }>(r)),
  leadModels: (provider) => fetch(`/api/lead/models?${new URLSearchParams({ provider })}`).then((r) => json<{ provider: LeadProvider; models: LeadModel[] }>(r)),
};
