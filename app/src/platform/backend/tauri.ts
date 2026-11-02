import type { ActionResult, AgentStatus, Backend, BudgetReport, Invoke, LeadAttachment, LeadDispatched, LeadEvent, LeadShow, ProjectCheck, ProjectList, ReviewStarted, TokenReport } from "./types.ts";
import type { About, UpdateCheck, UpdateRelease } from "../updates.ts";

/** project_O.exe: the shell's Tauri commands (app/src-tauri/src/commands), JSON text parsed here. */
export function tauriBackend(invoke: Invoke): Backend {
  return {
    kind: "tauri",
    agents: (clean) => invoke("agents_status", { clean }) as Promise<AgentStatus[]>,
    install: (id) => invoke("agent_install", { id }) as Promise<ActionResult>,
    login: (id, clean) => invoke("agent_login", { id, clean }) as Promise<ActionResult>,
    openExternal: (url) => invoke("open_external", { url }) as Promise<void>,
    about: () => invoke("about") as Promise<About>,
    checkUpdate: () => invoke("update_check") as Promise<UpdateCheck>,
    pendingUpdate: () => invoke("update_pending") as Promise<UpdateRelease | null>,
    installUpdate: (tag) => invoke("update_install", { tag }) as Promise<void>,
    cancelUpdate: () => invoke("update_cancel") as Promise<void>,
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
    projects: () => invoke("projects_list") as Promise<ProjectList>,
    projectsAdd: (path) => invoke("projects_add", { path }) as Promise<ProjectList>,
    projectsRemove: (path) => invoke("projects_remove", { path }) as Promise<ProjectList>,
    projectsNew: (name) => invoke("projects_new", { name }) as Promise<ProjectList & { created: string }>,
    projectsRoot: ({ path, reset, create }) => invoke("projects_root", { path: path ?? null, reset: Boolean(reset), create: Boolean(create) }) as Promise<ProjectList>,
    projectsCheck: (paths) => invoke("projects_check", { paths }) as Promise<ProjectCheck[]>,
    // The native picker opens from Rust; the screen gets the chosen path or null.
    pickFolder: () => invoke("pick_folder") as Promise<string | null>,
  };
}
