import type { ActionResult, AgentStatus, Backend, BudgetReport, LeadAttachment, LeadDispatched, LeadEvent, LeadShow, ReviewStarted, TokenReport, Invoke } from "./types.ts";

/** project_O.exe: the shell's Tauri commands (app/src-tauri/src/commands), JSON text parsed here. */
export function tauriBackend(invoke: Invoke): Backend {
  return {
    kind: "tauri",
    agents: (clean) => invoke("agents_status", { clean }) as Promise<AgentStatus[]>,
    install: (id) => invoke("agent_install", { id }) as Promise<ActionResult>,
    login: (id, clean) => invoke("agent_login", { id, clean }) as Promise<ActionResult>,
    openExternal: (url) => invoke("open_external", { url }) as Promise<void>,
    about: () => invoke("about") as Promise<{ version: string; home: string; agentsHome: string }>,
    review: (project, pr, clean) => invoke("review", { project, pr, clean }) as Promise<ReviewStarted>,
    budget: async () => JSON.parse((await invoke("budget")) as string) as BudgetReport,
    setBudgetPercent: async (percent) => JSON.parse((await invoke("budget_set_percent", { percent })) as string) as BudgetReport,
    tokens: async (project) => JSON.parse((await invoke("tokens", { project: project ?? null })) as string) as TokenReport,
    leadShow: (project) => invoke("lead_show", { project: project ?? null }) as Promise<LeadShow>,
    leadNew: (project) => invoke("lead_new", { project }) as Promise<{ threadId: string }>,
    // The bytes go as the raw body; the project and name ride percent-encoded in headers.
    leadAttach: async (project, file) =>
      invoke("lead_attach", new Uint8Array(await file.arrayBuffer()), {
        headers: { "x-project": encodeURIComponent(project), "x-name": encodeURIComponent(file.name) },
      }) as Promise<LeadAttachment>,
    leadSend: (r) =>
      invoke("lead_send", { project: r.project, provider: r.provider, effort: r.effort ?? null, text: r.text, attachments: r.attachments, clean: r.clean }) as Promise<{
        threadId: string | null;
        events: LeadEvent[];
      }>,
    leadDispatch: (planId, clean) => invoke("lead_dispatch", { planId, clean }) as Promise<LeadDispatched>,
    leadStop: (project) => invoke("lead_stop", { project: project ?? null }) as Promise<{ stopped: boolean }>,
  };
}
