/** The machine side's contract (`Backend`) and the JSON shapes it answers with, as the `o` commands print them. */
import type { LeadAttachment, LeadEvent, LeadModel, LeadProvider, LeadShow } from "../../../../src/core/lead.ts";
import type { ListedProject, ProjectCheck, ProjectList } from "../../../../src/core/projects.ts";
import type { About, UpdateCheck, UpdateRelease, UpdateStatus } from "../updates.ts";

export type { LeadAttachment, LeadEvent, LeadModel, LeadProvider, LeadShow, ListedProject, ProjectCheck, ProjectList };

export type AgentId = "claude" | "codex" | "agy" | "opencode" | "devin";

export interface IssueRef { number: number; updatedAt: string }
export interface Issue extends IssueRef { title: string; state: string; url: string }
export interface Resolution { runId: string; cardId: string; evidence: string }
export interface Finding {
  repo: string; runId: string; cardId: string; sha: string; summary: string;
  reproduction: string; evidence: string[]; classification: string; issue: IssueRef | null;
  resolution?: Resolution;
}
export interface FindingRow { id: number; finding: Finding }

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
  /** With the project_O profile: why its cards cannot work until a setup and a passing check (Codex's Windows sandbox, ADR-0040 개정 1). */
  profileNotReady?: string;
  /** Names of present API authentication environment variables, never their values. */
  apiKeyEnv?: string[];
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
  lead?: { requested?: number; unreported?: number; incomplete?: number };
  /** Missing in older O versions; reported totals may omit provider usage. */
  summary?: {
    reported: TokenCounts;
    reportedCards: number;
    unreportedCards: number;
    incompleteCards: number;
    cachePercent: number | null;
  };
}

export interface ResourceLease {
  id: string;
  runId: string;
  cardId: string;
  resources: unknown;
  ownerPid: number | null;
  ownerStarted: string | null;
  workerPid: number | null;
  workerStarted: string | null;
  spawnIntent: boolean | null;
  exitConfirmed: boolean | null;
  /** The card's named kill-on-close job (Windows, ADR-0039); absent or null for older leases. */
  jobName?: string | null;
  state: "recoverable" | "unknown";
}

export type WorkspaceChanges = { workspace: string; note: string; baselineRunId?: string } & (
  | { gitCommit: string; changes: { changed: string[]; staged: string[]; unstaged: string[]; unmerged: string[]; untracked: string[]; ignored: string[]; masked: { path: string; skipWorktree: boolean; assumeUnchanged: boolean }[] } }
  | { copyBase: string; changes: { path: string; kind: "added" | "deleted" | "modified" }[] }
);

export interface Backend {
  kind: "tauri" | "dev" | "demo";
  resources(): Promise<ResourceLease[]>;
  recoverResource(id: string): Promise<void>;
  followup(run: string, card: string, text: string): Promise<{ runId: string }>;
  changes(run: string, card: string): Promise<WorkspaceChanges>;
  /** The person's answer to a card's question, sent as the lead on the run's team channel (`o msg send --as lead`). */
  reply(run: string, card: string, text: string, messageId?: string): Promise<void>;
  /** Flashes the taskbar button while the window is not in front (desktop only; elsewhere nothing). */
  attention(): Promise<void>;
  /** Agent status with the clean test profiles (`clean: true`) or the normal sign-in. */
  agents(clean: boolean): Promise<AgentStatus[]>;
  agentsCatalog(refresh?: boolean): Promise<AgentCatalogEntry[]>;
  /** Runs the official installer for one CLI and returns when it finishes. */
  install(id: AgentId): Promise<ActionResult>;
  /** Opens the official sign-in for one CLI (a console window and/or the browser). */
  login(id: AgentId, clean: boolean): Promise<ActionResult>;
  /** Sets up the project_O profile with no window and checks it with one real card-like command (Codex's Windows sandbox; one small model call; `o agents setup`, ADR-0040 개정 1). Takes minutes at most. */
  setup(id: AgentId): Promise<ActionResult>;
  openTerminal(): Promise<ActionResult>;
  /** Opens a web page in the person's browser. */
  openExternal(url: string): Promise<void>;
  imageRead(project: string, path: string, run?: string, card?: string): Promise<{ mime: string; bytes: number[] }>;
  imageOpen(project: string, path: string, folder: boolean, run?: string, card?: string): Promise<void>;
  schedules(): Promise<ScheduleJob[]>;
  scheduleCreate(request: ScheduleRequest): Promise<ScheduleJob>;
  scheduleDelete(id: string): Promise<void>;
  scheduleSetEnabled(id: string, enabled: boolean): Promise<ScheduleJob>;
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
  issuesSearch(repo: string, query: string): Promise<{ issues: Issue[]; possiblyTruncated: boolean }>;
  issuesList(repo: string): Promise<FindingRow[]>;
  issuesRecord(finding: Finding): Promise<{ id: number }>;
  issuesClassify(id: number, classification: string, issue: IssueRef | null): Promise<{ id: number }>;
  issuesResolve(id: number, resolution: Resolution | null): Promise<{ id: number }>;
  /** Today's usage budget per provider from saved readings (`o budget --json --cached`). */
  budget(): Promise<BudgetReport>;
  /** Settings → 에이전트: saves the day's allowance (the machine refuses anything but 12–14). */
  setBudgetPercent(percent: BudgetPercent): Promise<BudgetReport>;
  /** Turns the daily budget on or off (`o budget --on|--off`) and returns the new report. */
  setBudgetEnabled(enabled: boolean): Promise<BudgetReport>;
  /** The RAM cap on card starts (`o ram --json`, ADR-0037). */
  ram(): Promise<RamReport>;
  /** Saves the RAM cap's switch and/or percent (50–95) and returns the new report. */
  setRam(change: RamChange): Promise<RamReport>;
  /** Whether a CLI auth variable only warns or keeps cards, leads and reviews from starting (`o billing --json`, ADR-0035 개정 1). */
  billing(): Promise<BillingReport>;
  /** Saves the API billing mode (`o billing --warn|--subscription`) and returns the new report. */
  setBilling(mode: BillingMode): Promise<BillingReport>;
  yolo(): Promise<YoloReport>;
  setYolo(change: YoloChange): Promise<YoloReport>;
  /** Tokens per card, averaged per way of running (`o tokens --json`), for one project or all. */
  tokens(project?: string): Promise<TokenReport>;

  // The lead chat (ADR-0008). `leadDispatch` starts a plan or explicitly retries a failed run.

  /** One of the project's open lead threads (or the current one) and every project that has one (`o lead show --json`, ADR-0034). */
  leadShow(project?: string, thread?: string): Promise<LeadShow>;
  leadNote(threadId: string, change: import("../../../../src/core/lead.ts").DesignNoteChange): Promise<void>;
  /** Opens a new lead tab for the project ("새 대화", "새 설계"); rejects with the reason at 4 open. */
  leadNew(project: string): Promise<{ threadId: string }>;
  /** Takes a thread off the project's open tabs; its history stays. Rejects while it answers or its exit is unknown. */
  leadClose(threadId: string): Promise<void>;
  /** Makes an open tab the project's writer — the one that may edit files directly (ADR-0034). */
  leadWriter(threadId: string): Promise<void>;
  /** Saves one image or document for the project's thread; rejects with the reason. */
  leadAttach(project: string, file: File, thread?: string): Promise<LeadAttachment>;
  /** One message; resolves when the lead has answered, with the events the turn added. */
  leadSend(request: LeadSendRequest): Promise<{ threadId: string | null; events: LeadEvent[] }>;
  /** Runs a proposed plan after the policy check; rejects with the policy's reasons when refused. */
  leadDispatch(planId: string, clean: boolean): Promise<LeadDispatched>;
  /** The person sends the overall lead's proposed requests on to project leads (`o lead forward`, ADR-0036). */
  leadForward(proposalId: string): Promise<{ events: LeadEvent[] }>;
  /** The person's 허용/거부 for one action the lead waits on (`o lead decide`); only the person's click, never chat text. */
  leadDecide(id: string, allow: boolean): Promise<{ id: string; decision: "allow" | "deny" }>;
  /** Stops one lead thread while it answers (`thread`), or the project's open threads without one. */
  leadStop(project?: string, thread?: string): Promise<{ stopped: boolean }>;
  leadWake(runId: string): Promise<{ processed: boolean }>;
  leadRetry(runId: string, messageId: string): Promise<{ state: "queued" | "acknowledged" }>;
  leadAnswerApproval(runId: string, messageId: string, text: string): Promise<{ id: string }>;
  leadStopAll(project: string): Promise<{ stopped: number }>;
  /** The models a lead of this agent may name, from the CLI's own list (`o lead models`), cached a few minutes; rejects when the CLI could not list them. */
  leadModels(provider: LeadProvider): Promise<{ provider: LeadProvider; models: LeadModel[] }>;

  // The Graph tab (ADR-0033). `graphDeep` spends the person's subscription once, only after a click.

  /** `o graph view --json`: the 3D graph for every listed project (no `project`) or one folder. */
  graphView(project?: string): Promise<GraphView>;
  /**
   * `o graph part`: one project's slice of the overall graph (its node, areas and contains
   * links — no root). Missing on backends that cannot slice the overall graph (demo, old
   * dev servers); the overall scope then falls back to one `graphView()` call.
   */
  graphOverallPart?(project: string): Promise<GraphView>;
  /** `o graph build`: runs `grafly build` in the project; the parser's output is in `output`. */
  graphBuild(project: string): Promise<{ ok: boolean; output: string }>;
  /** `o graph deep`: one read-only LLM call naming the project's areas; rejects with the reason (budget spent, limit reached). */
  graphDeep(project: string, provider?: "codex" | "claude", model?: string): Promise<GraphDeepResult>;
  /** `o graph settings`: read (no `change`) or save the Graph tab's settings. */
  graphSettings(change?: Partial<GraphSettings>): Promise<GraphSettings>;

  // The project list (ADR-0009). Nothing here deletes a folder; refusals reject with the reason.

  /** The projects root, its projects and the registered folders (`o projects --json`). */
  projects(): Promise<ProjectList>;
  projectOpen(path: string): Promise<void>;
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

export interface ScheduleRequest {
  name: string;
  project: string;
  provider: "claude" | "codex" | "agy";
  text: string;
  start: string;
  repeat: "once" | "daily" | "weekly";
  clean: boolean;
  autoDispatch: boolean;
}
export type ScheduleJob = ScheduleRequest & { id: string; enabled: boolean; last?: { at: string; ok: boolean; error?: string } };

export interface RamReport {
  enabled: boolean;
  percent: number;
  /** This machine's used memory now; null where O cannot read it (the cap then does nothing). */
  used: number | null;
}
export type RamChange = { enabled?: boolean; percent?: number };

/** `warn` passes CLI auth variables and only warns; `subscription` refuses to start while one is set (ADR-0035 개정 1). */
export type BillingMode = "warn" | "subscription";
export interface BillingReport {
  mode: BillingMode;
}

export interface YoloReport {
  enabled: boolean;
  budgetEnabled: boolean;
  autoRun: { maxCards: number; perHour: number; dedupeMinutes: number };
}
export type YoloChange = { enabled?: boolean; maxCards?: number; perHour?: number; dedupeMinutes?: number };

export interface LeadSendRequest {
  project: string;
  /** The open tab to write to; absent goes to the project's current conversation (ADR-0034). */
  thread?: string;
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

/** `o graph view --json`: the whole workspace (root → projects → areas) or one project (project → folders → files). */
export interface GraphView {
  scope: "overall" | "project";
  /** The folder `scope: "project"` answers for. */
  project?: string;
  grafly: { installed: boolean; version?: string };
  /** A project's wiring.json exists (overall: at least one does). */
  indexed: boolean;
  nodes: GraphNode[];
  links: GraphLink[];
  /** `maxNodes` cut the graph: glowing nodes always stay. */
  truncated: boolean;
  /** When a Deep answer last named this graph's areas. */
  deep?: GraphDeepStamp;
}

/** The stamp a `graph deep` run leaves on the view. */
export interface GraphDeepStamp {
  at: string;
  provider: string;
  model: string;
}

export interface GraphNode {
  /** "o:root" | "p:<경로>" | "a:<경로>|<폴더>" | "d:<폴더>" | "f:<파일>". */
  id: string;
  label: string;
  kind: "root" | "project" | "area" | "file";
  project?: string;
  /** The project-relative path of a file or folder node. */
  path?: string;
  /** File or symbol count (1 or more). */
  size: number;
  /** "active": changed on disk now; "path": on its impact path. */
  glow?: "active" | "path";
  /** The Deep summary, when one named this area. */
  summary?: string;
  /** Project nodes: whether that project's index exists. */
  indexed?: boolean;
  /**
   * Screen-side only: why this project's slice of the overall graph did not load
   * (`graphOverallPart` rejected). Never comes from the backend.
   */
  error?: string;
}

export interface GraphLink {
  source: string;
  target: string;
  kind: "contains" | "calls" | "imports" | "references";
  /** How many edges between the same pair of files were folded into this one. */
  weight: number;
  /** On a glowing path. */
  glow?: boolean;
}

/** What file and area nodes take their colors from (`o graph settings --color-by`). */
export type GraphColorBy = "language" | "area";

/** `o graph settings --json` (<O 상태 폴더>/graph/settings.json). */
export interface GraphSettings {
  /** Glowing nodes, halo and brighter path links. */
  effects: boolean;
  /** Dots flowing along a glowing path. */
  particles: boolean;
  deepProvider: "codex" | "claude";
  /** "" = the CLI's own default model. */
  codexModel: string;
  claudeModel: string;
  /** The first-use "LLM 사용량이 차감됩니다" dialog before Deep. */
  warnDeep: boolean;
  /** Node cap per view (100–5000, default 1500). */
  maxNodes: number;
  /** Impact-path depth (1–4, default 2). */
  depth: number;
  /** Node coloring rule (default "language"); missing in answers from older `o`. */
  colorBy?: GraphColorBy;
}

/** `o graph deep --json`: the LLM's names and summaries for the project's areas. */
export interface GraphDeepResult {
  areas: { id: string; name: string; summary: string }[];
  provider: string;
  model: string;
  at: string;
  /** Tokens the CLI reported; null when it did not report them. */
  usage: { input: number | null; output: number | null };
}

/** Tauri's invoke: named arguments, or raw bytes with headers (attachments). */
export type Invoke = (cmd: string, args?: Record<string, unknown> | Uint8Array, options?: { headers?: Record<string, string> }) => Promise<unknown>;

declare global {
  interface Window {
    __TAURI_INTERNALS__?: { invoke: Invoke };
  }
}
