/**
 * What the screens ask of the machine beyond reading runs: which agent CLIs are
 * installed and signed in, and the official install and sign-in steps. In the
 * desktop app (project_O.exe) these are Tauri commands; under the dev server they are
 * /api routes (app/dev/liveRuns.ts); in the recorded demo they are a fixed answer.
 */
import { DESKTOP_UPDATES_ONLY, type About, type UpdateCheck, type UpdateRelease } from "./updates.ts";

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

/** What `o review <pr> --post --detach` answers: the review run started in the background. */
export interface ReviewStarted {
  ok: boolean;
  message: string;
  runId?: string;
  pr?: number;
  provider?: AgentId;
  /** The PR head commit being reviewed; the comment is posted only while it is still the head. */
  head?: string;
  clean?: boolean;
}

/** A PR number or a github.com pull address: the only forms `o review` takes. */
export const PR_SELECTOR = /^(?:[1-9]\d{0,6}|https:\/\/github\.com\/[\w.-]+\/[\w.-]+\/pull\/[1-9]\d{0,6})$/;

/** How a provider's spend today compares with how much of the day is left (ADR-0004). */
export type BudgetState = "unknown" | "ok" | "behind" | "farBehind" | "over";

/** One provider in `o budget --json`: shares are 0 to 1, weeklyUsed/allowance/spent percent points of the weekly window. */
export interface ProviderBudget {
  provider: AgentId;
  state: BudgetState;
  allowance: number;
  group: string | null;
  weeklyUsed: number | null;
  spent: number | null;
  remaining: number | null;
  remainingHours: number | null;
  timeShare: number | null;
  remainingShare: number | null;
  pace: number | null;
  stepsDown: number;
  /** What each effort becomes on a card started now (`from` null: a card that named none). */
  efforts: { from: string | null; to: string }[];
  countedFrom: "dayStart" | "firstReading" | "weeklyReset" | null;
  periodEnd: string | null;
  weeklyResetsAt: string | null;
  readAt: string | null;
  /** One Korean line for people. */
  message: string;
}

export interface BudgetReport {
  at: string;
  /** The day's allowance, percent of the weekly window (12–14). */
  percent: number;
  /** When `o usage` last asked Codex and Antigravity; empty when never. */
  usageAt: string;
  providers: ProviderBudget[];
}

export const BUDGET_PERCENTS = [12, 13, 14] as const;
export type BudgetPercent = (typeof BUDGET_PERCENTS)[number];

/** Tokens as the CLIs report them: fresh input (cache writes included), cached input, output. */
export interface TokenCounts {
  input: number;
  cached: number;
  output: number;
  total: number;
}

/** One way of running cards (`o tokens --json`): the default harness or clean agents, with or without shared context. */
export interface TokenGroup {
  key: "normal" | "normal+context" | "clean" | "clean+context";
  label: string;
  clean: boolean;
  context: boolean;
  runs: number;
  cards: number;
  tokens: TokenCounts;
  perCard: TokenCounts;
  byProvider: { provider: AgentId; cards: number; perCard: TokenCounts }[];
}

export interface TokenReport {
  groups: TokenGroup[];
  compare: { baseline: TokenGroup | null; shared: TokenGroup | null };
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
  about(): Promise<About>;
  checkUpdate(): Promise<UpdateCheck>;
  pendingUpdate(): Promise<UpdateRelease | null>;
  installUpdate(tag: string): Promise<void>;
  cancelUpdate(): Promise<void>;
  /**
   * Asks an agent to review a PR of the project folder and comment the review on it
   * (`o review <pr> --post` there). It never approves or merges. Rejects with the
   * reason (gh failed, policy refused) when the review did not start.
   */
  review(project: string, pr: string, clean: boolean): Promise<ReviewStarted>;
  /** Today's usage budget per provider from saved readings (`o budget --json --cached`). */
  budget(): Promise<BudgetReport>;
  /** Settings → 에이전트: saves the day's allowance (the machine refuses anything but 12–14). */
  setBudgetPercent(percent: BudgetPercent): Promise<BudgetReport>;
  /** Tokens per card, averaged per way of running (`o tokens --json`), for one project or all. */
  tokens(project?: string): Promise<TokenReport>;
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

/** The preview's budget: Codex a little ahead of the day, Antigravity spending fast, Claude unread. */
function demoBudget(percent: number): BudgetReport {
  const at = new Date().toISOString();
  const known = (provider: AgentId, spent: number, state: BudgetState, stepsDown: number, efforts: ProviderBudget["efforts"], message: string): ProviderBudget => ({
    provider,
    state,
    allowance: percent,
    group: null,
    weeklyUsed: 30 + spent,
    spent,
    remaining: Math.max(0, percent - spent),
    remainingHours: 9.5,
    timeShare: 0.4,
    remainingShare: Math.max(0, percent - spent) / percent,
    pace: Math.max(0, percent - spent) / percent / 0.4,
    stepsDown,
    efforts,
    countedFrom: "dayStart",
    periodEnd: at,
    weeklyResetsAt: at,
    readAt: at,
    message,
  });
  return {
    at,
    percent,
    usageAt: at,
    providers: [
      { ...known("claude", 0, "unknown", 0, [], "claude 주간 사용량 기록이 없어 예산을 조절하지 않습니다"), weeklyUsed: null, spent: null, remaining: null, pace: null, countedFrom: null },
      known("codex", 4.2, "ok", 0, [], `codex 오늘 주간의 4.2%p 사용 / 하루 예산 ${percent}%p, 9.5시간 남음 — 여유 있음`),
      known("agy", 9.1, "behind", 1, [{ from: null, to: "low" }, { from: "high", to: "medium" }], `agy 오늘 주간의 9.1%p 사용 / 하루 예산 ${percent}%p, 9.5시간 남음 — 쓰는 속도가 빨라 effort를 한 단계 낮춥니다`),
    ],
  };
}

let demoPercent = 13;

const demoTokens = (input: number, cached: number, output: number): TokenCounts => ({ input, cached, output, total: input + cached + output });
const demoGroup = (key: TokenGroup["key"], label: string, runs: number, cards: number, perCard: TokenCounts): TokenGroup => ({
  key,
  label,
  clean: key.startsWith("clean"),
  context: key.endsWith("context"),
  runs,
  cards,
  tokens: demoTokens(perCard.input * cards, perCard.cached * cards, perCard.output * cards),
  perCard,
  byProvider: [{ provider: "codex", cards, perCard }],
});
const DEMO_BASELINE = demoGroup("normal", "기본 하네스", 3, 9, demoTokens(18_400, 41_200, 3_100));
const DEMO_SHARED = demoGroup("clean+context", "깨끗한+세션 공유", 3, 9, demoTokens(6_900, 12_800, 2_700));

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
  about: async () => ({ version: "preview", home: "", agentsHome: "", commit: "", builtAt: "" }),
  checkUpdate: async () => ({ release: null, message: DESKTOP_UPDATES_ONLY }),
  pendingUpdate: async () => null,
  installUpdate: async () => { throw new Error(DESKTOP_UPDATES_ONLY); },
  cancelUpdate: async () => {},
  review: async () => ({ ok: false, message: "미리보기에서는 검토를 요청하지 않습니다" }),
  budget: async () => demoBudget(demoPercent),
  setBudgetPercent: async (percent) => demoBudget((demoPercent = percent)),
  tokens: async () => ({ groups: [DEMO_BASELINE, DEMO_SHARED], compare: { baseline: DEMO_BASELINE, shared: DEMO_SHARED } }),
};

/** The desktop app when Tauri is present, the dev server's /api otherwise; `demo` forces the recorded preview. */
export function backend(demo = false): Backend {
  if (demo) return demoBackend;
  const tauri = window.__TAURI_INTERNALS__;
  return tauri ? tauriBackend(tauri.invoke) : devBackend;
}
