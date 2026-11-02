/**
 * What the screens ask of the machine beyond reading runs: which agent CLIs are
 * installed and signed in, and the official install and sign-in steps. In the
 * desktop app (project_O.exe) these are Tauri commands; under the dev server they are
 * /api routes (app/dev/liveRuns.ts); in the recorded demo they are a fixed answer.
 */
export type AgentId = "claude" | "codex" | "agy";

export interface AgentStatus {
  id: AgentId;
  /** "Claude Code", "Codex", "Antigravity". */
  name: string;
  installed: boolean;
  /** What `<cli> --version` printed, when installed. */
  version?: string;
  /** Where the CLI was found on PATH. */
  path?: string;
  /** Signed in, per the CLI's own status command; null when it cannot tell. */
  loggedIn: boolean | null;
  /** The official way to install it, in words a person can follow ("npm install -g …"). */
  installHint: string;
  /** Whether Project O can run the install itself (otherwise it opens installUrl). */
  canInstall: boolean;
  installUrl: string;
}

export interface ActionResult {
  ok: boolean;
  /** One or two lines for the person; the full log goes to `log`. */
  message: string;
  log?: string;
}

export interface Backend {
  kind: "tauri" | "dev" | "demo";
  /** Agent status with the clean test profiles (`clean: true`) or the normal sign-in. */
  agents(clean: boolean): Promise<AgentStatus[]>;
  /** Runs the official installer for one CLI and returns when it finishes. */
  install(id: AgentId): Promise<ActionResult>;
  /** Opens the official sign-in for one CLI (a console window and/or the browser). */
  login(id: AgentId, clean: boolean): Promise<ActionResult>;
  /** Opens a web page in the person's browser. */
  openExternal(url: string): Promise<void>;
  /** App version and where O keeps its state, for Settings → About. */
  about(): Promise<{ version: string; home: string; agentsHome: string }>;
}

declare global {
  interface Window {
    __TAURI_INTERNALS__?: { invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown> };
  }
}

function tauriBackend(invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>): Backend {
  return {
    kind: "tauri",
    agents: (clean) => invoke("agents_status", { clean }) as Promise<AgentStatus[]>,
    install: (id) => invoke("agent_install", { id }) as Promise<ActionResult>,
    login: (id, clean) => invoke("agent_login", { id, clean }) as Promise<ActionResult>,
    openExternal: (url) => invoke("open_external", { url }) as Promise<void>,
    about: () => invoke("about") as Promise<{ version: string; home: string; agentsHome: string }>,
  };
}

async function json<T>(res: Response): Promise<T> {
  if (!res.ok) throw new Error((await res.text()) || `HTTP ${res.status}`);
  return (await res.json()) as T;
}

const devBackend: Backend = {
  kind: "dev",
  agents: (clean) => fetch(`/api/agents?clean=${clean ? 1 : 0}`).then((r) => json<AgentStatus[]>(r)),
  install: (id) => fetch(`/api/agents/${id}/install`, { method: "POST" }).then((r) => json<ActionResult>(r)),
  login: (id, clean) => fetch(`/api/agents/${id}/login?clean=${clean ? 1 : 0}`, { method: "POST" }).then((r) => json<ActionResult>(r)),
  openExternal: async (url) => void window.open(url, "_blank", "noopener"),
  about: () => fetch("/api/about").then((r) => json<{ version: string; home: string; agentsHome: string }>(r)),
};

/** The recorded demo: every agent installed and signed in, nothing really runs. */
const demoBackend: Backend = {
  kind: "demo",
  agents: async () => [
    { id: "claude", name: "Claude Code", installed: true, version: "demo", loggedIn: true, installHint: "", canInstall: false, installUrl: "" },
    { id: "codex", name: "Codex", installed: true, version: "demo", loggedIn: true, installHint: "", canInstall: false, installUrl: "" },
    { id: "agy", name: "Antigravity", installed: true, version: "demo", loggedIn: true, installHint: "", canInstall: false, installUrl: "" },
  ],
  install: async () => ({ ok: false, message: "미리보기에서는 설치하지 않습니다" }),
  login: async () => ({ ok: false, message: "미리보기에서는 연결하지 않습니다" }),
  openExternal: async (url) => void window.open(url, "_blank", "noopener"),
  about: async () => ({ version: "preview", home: "", agentsHome: "" }),
};

/** The desktop app when Tauri is present, the dev server's /api otherwise; `demo` forces the recorded preview. */
export function backend(demo = false): Backend {
  if (demo) return demoBackend;
  const tauri = window.__TAURI_INTERNALS__;
  return tauri ? tauriBackend(tauri.invoke) : devBackend;
}
