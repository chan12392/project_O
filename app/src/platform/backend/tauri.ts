import type { ActionResult, AgentStatus, Backend, BudgetReport, ReviewStarted, TokenReport } from "./types.ts";
import type { About, UpdateCheck, UpdateRelease } from "../updates.ts";

/** project_O.exe: the shell's Tauri commands (app/src-tauri/src/commands), JSON text parsed here. */
export function tauriBackend(invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>): Backend {
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
  };
}
