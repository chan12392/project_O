import type { ActionResult, AgentStatus, Backend, BudgetReport, GraphDeepResult, GraphSettings, GraphView, LeadAttachment, LeadDispatched, LeadEvent, LeadModel, LeadProvider, LeadShow, ProjectCheck, ProjectList, ReviewStarted, TokenReport } from "./types.ts";
import { DESKTOP_UPDATES_ONLY, NO_UPDATE_STATUS, type About, type UpdateCheck } from "../updates.ts";

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
  resources: () => fetch("/api/resources").then((r) => json<{ leases: Awaited<ReturnType<Backend["resources"]>> }>(r)).then((r) => r.leases),
  recoverResource: (id) => fetch(`/api/resources/recover?id=${encodeURIComponent(String(id))}`, { method: "POST" }).then((r) => json<unknown>(r)).then(() => {}),
  followup: (run, card, text) => fetch(`/api/lead/followup?${new URLSearchParams({ run, card })}`, { method: "POST", body: text }).then((r) => json<{ runId: string }>(r)),
  changes: (run, card) => fetch(`/api/changes?${new URLSearchParams({ run, card })}`).then((r) => json<Awaited<ReturnType<Backend["changes"]>>>(r)),
  reply: (run, card, text, messageId) => fetch(`/api/lead/reply?${new URLSearchParams({ run, card, ...(messageId ? { message: messageId } : {}) })}`, { method: "POST", body: text }).then((r) => json<unknown>(r)).then(() => {}),
  attention: async () => {},
  imageRead: (project, path, run, card) => fetch(`/api/lead/image/read?${new URLSearchParams({ project, path, run: run ?? "-", card: card ?? "-" })}`).then((r) => json<Awaited<ReturnType<Backend["imageRead"]>>>(r)),
  imageOpen: (project, path, folder, run, card) => fetch(`/api/lead/image/${folder ? "folder" : "open"}?${new URLSearchParams({ project, path, run: run ?? "-", card: card ?? "-" })}`, { method: "POST" }).then((r) => json<unknown>(r)).then(() => {}),
  schedules: () => fetch("/api/lead/schedule").then((r) => json<Awaited<ReturnType<Backend["schedules"]>>>(r)),
  scheduleCreate: (request) => fetch("/api/lead/schedule", { method: "POST", body: JSON.stringify(request) }).then((r) => json<Awaited<ReturnType<Backend["scheduleCreate"]>>>(r)),
  scheduleDelete: (id) => fetch(`/api/lead/schedule/delete?id=${encodeURIComponent(id)}`, { method: "POST" }).then((r) => json<unknown>(r)).then(() => {}),
  scheduleSetEnabled: (id, enabled) => fetch(`/api/lead/schedule/${enabled ? "enable" : "disable"}?id=${encodeURIComponent(id)}`, { method: "POST" }).then((r) => json<Awaited<ReturnType<Backend["scheduleSetEnabled"]>>>(r)),
  agents: (clean) => fetch(`/api/agents?clean=${clean ? 1 : 0}`).then((r) => json<AgentStatus[]>(r)),
  agentsCatalog: (refresh = false) => fetch(`/api/agents/catalog${refresh ? "?refresh=1" : ""}`).then((r) => json<Awaited<ReturnType<Backend["agentsCatalog"]>>>(r)),
  install: (id) => fetch(`/api/agents/${id}/install`, { method: "POST" }).then((r) => json<ActionResult>(r)),
  login: (id, clean) => fetch(`/api/agents/${id}/login?clean=${clean ? 1 : 0}`, { method: "POST" }).then((r) => json<ActionResult>(r)),
  openTerminal: () => fetch("/api/agents/terminal", { method: "POST" }).then((r) => json<ActionResult>(r)),
  openExternal: async (url) => void window.open(url, "_blank", "noopener"),
  about: () => fetch("/api/about").then((r) => json<About>(r)),
  checkUpdate: () => fetch("/api/updates").then((r) => json<UpdateCheck>(r)),
  pendingUpdate: async () => null,
  installUpdate: async () => { throw new Error(DESKTOP_UPDATES_ONLY); },
  cancelUpdate: async () => {},
  updateStatus: async () => NO_UPDATE_STATUS,
  review: async (project, pr, clean) => {
    const res = await fetch(`/api/review?${new URLSearchParams({ project, pr, clean: clean ? "1" : "0" })}`, { method: "POST" });
    const body = (await res.json().catch(() => undefined)) as (ReviewStarted & { error?: string }) | undefined;
    if (!res.ok || !body) throw new Error(body?.error ?? `HTTP ${res.status}`);
    return body;
  },
  issuesSearch: (repo, query) => fetch(`/api/issues/search?${new URLSearchParams({ repo, query })}`).then((r) => json<Awaited<ReturnType<Backend["issuesSearch"]>>>(r)),
  issuesList: (repo) => fetch(`/api/issues/list?${new URLSearchParams({ repo })}`).then((r) => json<Awaited<ReturnType<Backend["issuesList"]>>>(r)),
  issuesRecord: (finding) => fetch("/api/issues/record", { method: "POST", body: JSON.stringify(finding) }).then((r) => json<Awaited<ReturnType<Backend["issuesRecord"]>>>(r)),
  issuesClassify: (id, classification, issue) => fetch("/api/issues/classify", { method: "POST", body: JSON.stringify({ id, classification, issue }) }).then((r) => json<Awaited<ReturnType<Backend["issuesClassify"]>>>(r)),
  issuesResolve: (id, resolution) => fetch("/api/issues/resolve", { method: "POST", body: JSON.stringify({ id, resolution }) }).then((r) => json<Awaited<ReturnType<Backend["issuesResolve"]>>>(r)),
  budget: () => fetch("/api/budget").then((r) => json<BudgetReport>(r)),
  setBudgetPercent: (percent) => fetch(`/api/budget/percent?value=${percent}`, { method: "POST" }).then((r) => json<BudgetReport>(r)),
  setBudgetEnabled: (enabled) => fetch(`/api/budget/enabled?value=${enabled ? "on" : "off"}`, { method: "POST" }).then((r) => json<BudgetReport>(r)),
  yolo: () => fetch("/api/yolo").then((r) => json<Awaited<ReturnType<Backend["yolo"]>>>(r)),
  setYolo: (change) => fetch("/api/yolo", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(change) }).then((r) => json<Awaited<ReturnType<Backend["yolo"]>>>(r)),
  tokens: (project) => fetch(`/api/tokens${project ? `?project=${encodeURIComponent(project)}` : ""}`).then((r) => json<TokenReport>(r)),
  leadShow: (project) => fetch(`/api/lead/show${project ? `?project=${encodeURIComponent(project)}` : ""}`).then((r) => json<LeadShow>(r)),
  leadNote: (threadId, change) => fetch(`/api/lead/note?${new URLSearchParams({ thread: threadId })}`, { method: "POST", body: JSON.stringify(change) }).then((r) => json<unknown>(r)).then(() => {}),
  leadNew: (project) => fetch(`/api/lead/new?${new URLSearchParams({ project })}`, { method: "POST" }).then((r) => json<{ threadId: string }>(r)),
  leadAttach: (project, file) =>
    fetch(`/api/lead/attach?${new URLSearchParams({ project, name: file.name })}`, { method: "POST", body: file }).then((r) => json<LeadAttachment>(r)),
  leadSend: (request) =>
    fetch(`/api/lead/send?${new URLSearchParams({ project: request.project })}`, { method: "POST", body: JSON.stringify(request) }).then((r) =>
      json<{ threadId: string | null; events: LeadEvent[] }>(r),
    ),
  leadDispatch: (planId, clean) =>
    fetch(`/api/lead/dispatch?${new URLSearchParams({ plan: planId, clean: clean ? "1" : "0" })}`, { method: "POST" }).then((r) => json<LeadDispatched>(r)),
  leadDecide: (id, allow) => fetch(`/api/lead/decide?${new URLSearchParams({ id, decision: allow ? "allow" : "deny" })}`, { method: "POST" }).then((r) => json<{ id: string; decision: "allow" | "deny" }>(r)),
  leadStop: (project) => fetch(`/api/lead/stop${project ? `?project=${encodeURIComponent(project)}` : ""}`, { method: "POST" }).then((r) => json<{ stopped: boolean }>(r)),
  leadStopAll: (project) => fetch(`/api/lead/stop-all?project=${encodeURIComponent(project)}`, { method: "POST" }).then((r) => json<{ stopped: number }>(r)),
  leadWake: (runId) => fetch(`/api/lead/wake?run=${encodeURIComponent(runId)}`, { method: "POST" }).then((r) => json<{ processed: boolean }>(r)),
  leadRetry: (runId, messageId) => fetch(`/api/lead/retry?${new URLSearchParams({ run: runId, message: messageId })}`, { method: "POST" }).then((r) => json<{ state: "queued" | "acknowledged" }>(r)),
  leadAnswerApproval: (runId, messageId, text) => fetch(`/api/lead/answer-approval?${new URLSearchParams({ run: runId, message: messageId })}`, { method: "POST", body: text }).then((r) => json<{ id: string }>(r)),
  leadModels: (provider) => fetch(`/api/lead/models?${new URLSearchParams({ provider })}`).then((r) => json<{ provider: LeadProvider; models: LeadModel[] }>(r)),
  graphView: (project) => fetch(`/api/graph/view${project ? `?project=${encodeURIComponent(project)}` : ""}`).then((r) => json<GraphView>(r)),
  graphBuild: (project) => fetch(`/api/graph/build?${new URLSearchParams({ project })}`, { method: "POST" }).then((r) => json<{ ok: boolean; output: string }>(r)),
  graphDeep: async (project, provider, model) =>
    fetch(`/api/graph/deep?${new URLSearchParams({ project })}`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      // Read lazily: data/settings.ts touches window at import, and tests load this module under Node.
      body: JSON.stringify({ provider: provider ?? null, model: model ?? null, clean: (await import("../../data/settings.ts")).settingsNow().clean }),
    }).then((r) => json<GraphDeepResult>(r)),
  graphSettings: (change) =>
    change === undefined
      ? fetch("/api/graph/settings").then((r) => json<GraphSettings>(r))
      : fetch("/api/graph/settings", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(change) }).then((r) => json<GraphSettings>(r)),
  projects: () => fetch("/api/projects").then((r) => json<ProjectList>(r)),
  projectOpen: (path) => fetch(`/api/projects/open?${new URLSearchParams({ path })}`, { method: "POST" }).then((r) => json<unknown>(r)).then(() => {}),
  projectsAdd: (path) => fetch(`/api/projects/add?${new URLSearchParams({ path })}`, { method: "POST" }).then((r) => json<ProjectList>(r)),
  projectsRemove: (path) => fetch(`/api/projects/remove?${new URLSearchParams({ path })}`, { method: "POST" }).then((r) => json<ProjectList>(r)),
  projectsNew: (name) => fetch(`/api/projects/new?${new URLSearchParams({ name })}`, { method: "POST" }).then((r) => json<ProjectList & { created: string }>(r)),
  projectsRoot: ({ path, reset, create }) =>
    fetch(`/api/projects/root?${new URLSearchParams({ ...(path ? { path } : {}), reset: reset ? "1" : "0", create: create ? "1" : "0" })}`, { method: "POST" }).then((r) => json<ProjectList>(r)),
  projectsCheck: (paths) => fetch("/api/projects/check", { method: "POST", body: JSON.stringify(paths) }).then((r) => json<ProjectCheck[]>(r)),
  // No native picker in the browser: the screen asks for the path as text.
};
