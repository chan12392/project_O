/** The machine side's contract (`Backend`) and the JSON shapes it answers with, as the `o` commands print them. */
import type { LeadAttachment, LeadEvent, LeadModel, LeadProvider, LeadShow } from "../../../../src/core/lead.ts";
import type { ListedProject, ProjectCheck, ProjectList } from "../../../../src/core/projects.ts";
import type { About, UpdateCheck, UpdateRelease, UpdateStatus } from "../updates.ts";

export type { LeadAttachment, LeadEvent, LeadModel, LeadProvider, LeadShow, ListedProject, ProjectCheck, ProjectList };

export type AgentId = "claude" | "codex" | "agy" | "opencode" | "devin";

export interface AgentCatalogEntry {
  id: string;
  name: string;
  detected: boolean;
  version?: string;
  status: "connected" | "planned" | "unsupported";
  installUrl?: string;
  profileSupported: boolean;
  note: string;
}

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
  profileUnsupported?: string;
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

/** How a provider's spend today compares with how much of the day is left (ADR-0004); "off" while the daily budget is switched off (ADR-0013). */
export type BudgetState = "unknown" | "ok" | "behind" | "farBehind" | "over" | "off" | "notApplied";

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
  /** The daily budget switch (`o budget --on|--off`); off, nothing is refused or lowered and every provider is "off". */
  enabled: boolean;
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
  agentsCatalog(): Promise<AgentCatalogEntry[]>;
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
  /** Whether updates can be checked here (gh) and how the last background check went. */
  updateStatus(): Promise<UpdateStatus>;
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
  /** Turns the daily budget on or off (`o budget --on|--off`) and returns the new report. */
  setBudgetEnabled(enabled: boolean): Promise<BudgetReport>;
  yolo(): Promise<YoloReport>;
  setYolo(change: YoloChange): Promise<YoloReport>;
  /** Tokens per card, averaged per way of running (`o tokens --json`), for one project or all. */
  tokens(project?: string): Promise<TokenReport>;

  // The lead chat (ADR-0008). The lead only reads and proposes; `leadDispatch` runs a plan once.

  /** The project's current lead thread and every project that has one (`o lead show --json`). */
  leadShow(project?: string): Promise<LeadShow>;
  /** Starts a new thread for the project ("새 대화"). */
  leadNew(project: string): Promise<{ threadId: string }>;
  /** Saves one image or document for the project's current thread; rejects with the reason. */
  leadAttach(project: string, file: File): Promise<LeadAttachment>;
  /** One message; resolves when the lead has answered, with the events the turn added. */
  leadSend(request: LeadSendRequest): Promise<{ threadId: string | null; events: LeadEvent[] }>;
  /** Runs a proposed plan after the policy check; rejects with the policy's reasons when refused. */
  leadDispatch(planId: string, clean: boolean): Promise<LeadDispatched>;
  /** Stops the project's lead while it answers. */
  leadStop(project?: string): Promise<{ stopped: boolean }>;
  leadStopAll(project: string): Promise<{ stopped: number }>;
  /** The models a lead of this agent may name, from the CLI's own list (`o lead models`), cached a few minutes; rejects when the CLI could not list them. */
  leadModels(provider: LeadProvider): Promise<{ provider: LeadProvider; models: LeadModel[] }>;

  // The project list (ADR-0009). Nothing here deletes a folder; refusals reject with the reason.

  /** The projects root, its projects and the registered folders (`o projects --json`). */
  projects(): Promise<ProjectList>;
  /** Registers a folder outside the root (the same rules as a card's project folder). */
  projectsAdd(path: string): Promise<ProjectList>;
  /** Takes a registered folder off the list; the folder stays. */
  projectsRemove(path: string): Promise<ProjectList>;
  /** "새 프로젝트": a new empty folder in the root. */
  projectsNew(name: string): Promise<ProjectList & { created: string }>;
  /** Settings → 프로젝트: another root, back to the default (`reset`), and/or make it (`create`). */
  projectsRoot(change: { path?: string; reset?: boolean; create?: boolean }): Promise<ProjectList>;
  /** Which of these folders may be listed, each with the reason when not. */
  projectsCheck(paths: string[]): Promise<ProjectCheck[]>;
  /** The native folder picker (desktop only); null when cancelled or unavailable. */
  pickFolder?(): Promise<string | null>;
}

export interface YoloReport {
  enabled: boolean;
  budgetEnabled: boolean;
  autoRun: { maxCards: number; perHour: number; dedupeMinutes: number };
}
export type YoloChange = { enabled?: boolean; maxCards?: number; perHour?: number; dedupeMinutes?: number };

export interface LeadSendRequest {
  project: string;
  provider: LeadProvider;
  /** A model id (`--model`); none for the CLI's default. */
  model?: string;
  effort?: string;
  text: string;
  /** Paths `leadAttach` returned. */
  attachments: string[];
  clean: boolean;
}

export interface LeadDispatched {
  planId: string;
  runId: string;
  events: string;
  cards: string[];
  clean: boolean;
}

/** Tauri's invoke: named arguments, or raw bytes with headers (attachments). */
export type Invoke = (cmd: string, args?: Record<string, unknown> | Uint8Array, options?: { headers?: Record<string, string> }) => Promise<unknown>;

declare global {
  interface Window {
    __TAURI_INTERNALS__?: { invoke: Invoke };
  }
}
