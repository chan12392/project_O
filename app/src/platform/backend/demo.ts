import type { AgentId, Backend, BillingReport, BudgetReport, BudgetState, GraphDeepResult, GraphLink, GraphNode, GraphSettings, GraphView, LeadModel, LeadProvider, ListedProject, ProjectList, ProviderBudget, RamReport, TokenCounts, TokenGroup } from "./types.ts";
import { LEAD_TAB_MAX, type LeadShow, type LeadTab } from "../../../../src/core/lead.ts";
import { folderKey, sameFolder } from "../../../../src/core/projects.ts";
import { DESKTOP_UPDATES_ONLY, NO_UPDATE_STATUS } from "../updates.ts";

/** The preview's budget: Codex a little ahead of the day, Antigravity spending fast, Claude unread; all "off" while switched off. */
function demoBudget(percent: number, enabled: boolean): BudgetReport {
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
  const providers = [
    { ...known("claude", 0, "unknown", 0, [], "claude 주간 사용량 기록이 없어 예산을 조절하지 않습니다"), weeklyUsed: null, spent: null, remaining: null, pace: null, countedFrom: null },
    known("codex", 4.2, "ok", 0, [], `codex 오늘 주간의 4.2%p 사용 / 하루 예산 ${percent}%p, 9.5시간 남음 — 여유 있음`),
    known("agy", 9.1, "behind", 1, [{ from: null, to: "low" }, { from: "high", to: "medium" }], `agy 오늘 주간의 9.1%p 사용 / 하루 예산 ${percent}%p, 9.5시간 남음 — 쓰는 속도가 빨라 effort를 한 단계 낮춥니다`),
  ];
  const off = (b: ProviderBudget): ProviderBudget => ({
    ...b,
    state: "off",
    stepsDown: 0,
    efforts: [],
    message: `${b.provider} 하루 사용 예산 꺼짐${b.spent === null ? "" : ` — 오늘 주간의 ${b.spent}%p 사용`} (막거나 effort를 낮추지 않음)`,
  });
  return { at, enabled, percent, usageAt: at, providers: enabled ? providers : providers.map(off) };
}

let demoPercent = 13;
let demoEnabled = true;
const demoRam: RamReport = { enabled: true, percent: 70, used: 58 };
const demoBilling: BillingReport = { mode: "warn" };

export const LEAD_REFUSED = "미리보기에서는 총괄에 보내지 않습니다";

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

/** The preview's project list: the recorded runs' folders as a root, one test project and one registered folder. Changes last for this window only. */
const DEMO_ROOT = "C:/projects";
let demoProjects: ListedProject[] = [
  { path: `${DEMO_ROOT}/blog`, name: "blog", kind: "root" },
  { path: `${DEMO_ROOT}/project_O`, name: "project_O", kind: "root" },
  { path: `${DEMO_ROOT}/todo-app`, name: "todo-app", kind: "root" },
  { path: `${DEMO_ROOT}/test/o-sandbox`, name: "o-sandbox", kind: "test" },
  { path: `${DEMO_ROOT}/test/probe`, name: "probe", kind: "test" },
  { path: "D:/work/landing", name: "landing", kind: "registered" },
];
const demoList = (): ProjectList => ({ root: DEMO_ROOT, defaultRoot: true, rootExists: true, projects: demoProjects });

// The preview's open lead tabs (ADR-0034): each project seeds a writer and a waiting tab so the
// tab strip can be seen without a lead. +/×/writer change this map only; nothing reaches a lead.
const demoTabs = new Map<string, LeadTab[]>();
const demoOpen = (project: string): LeadTab[] => {
  const key = folderKey(project);
  let tabs = demoTabs.get(key);
  if (!tabs) {
    const tag = key.replace(/[^a-z0-9]+/g, "").slice(-6) || "demo";
    tabs = [
      { threadId: `20261108-090000-${tag}w`, title: "README 정리", state: "idle", busy: null, writer: true, startedAt: "2026-11-08T09:00:00.000Z" },
      { threadId: `20261108-093000-${tag}q`, title: "로그인 오류 조사", state: "waiting", busy: "claude", writer: false, startedAt: "2026-11-08T09:30:00.000Z" },
    ];
    demoTabs.set(key, tabs);
  }
  return tabs;
};
demoOpen(DEMO_ROOT);
for (const project of demoProjects) demoOpen(project.path);
let demoTabSeq = 0;

/** Every project with an open lead tab, with its aggregate state (waiting > working > idle). */
const demoLeadProjects = (): LeadShow["projects"] =>
  [...demoTabs.entries()].flatMap(([key, tabs]) => {
    if (!tabs.length) return [];
    const state: LeadTab["state"] = tabs.some((t) => t.state === "waiting") ? "waiting" : tabs.some((t) => t.state === "working") ? "working" : "idle";
    return [{ project: key, threadId: tabs[tabs.length - 1].threadId, state }];
  });
const ABSOLUTE = /^(?:[A-Za-z]:[\\/]|\/)/;
const nameOf = (path: string) => folderKey(path).split("/").filter(Boolean).pop() ?? path;

/**
 * The preview's model lists, in the shape `o lead models` prints (Claude's are its documented
 * aliases with the version each stands for). Antigravity lists every model at every thinking
 * level, so its list is the long one the menu has to scroll.
 */
const DEMO_MODELS: Record<LeadProvider, LeadModel[]> = {
  claude: [
    { id: "fable", label: "Fable 5.1", hint: "claude-fable-5-1" },
    { id: "opus", label: "Opus 5.5", hint: "claude-opus-5-5" },
    { id: "sonnet", label: "Sonnet 5.5", hint: "claude-sonnet-5-5" },
    { id: "haiku", label: "Haiku 5.5", hint: "claude-haiku-5-5" },
  ],
  codex: [
    { id: "gpt-6.1-sol", label: "GPT-6.1-Sol" },
    { id: "gpt-6.1-terra", label: "GPT-6.1 Terra" },
  ],
  agy: (
    [
      ["gemini-4-argon", "Gemini 4 Argon"],
      ["gemini-4-argon-pro", "Gemini 4 Argon Pro"],
      ["gemini-3.8-flash", "Gemini 3.8 Flash"],
      ["claude-opus-5-5", "Claude Opus 5.5"],
      ["claude-sonnet-5-5", "Claude Sonnet 5.5"],
    ] as const
  ).flatMap(([id, name]) => (["high", "medium", "low"] as const).map((level) => ({ id: `${id}-${level}`, label: `${name} (${level[0].toUpperCase()}${level.slice(1)})` }))),
};

/** The preview's graph settings (ADR-0033); saved for this window only. */
let demoGraphSettings: GraphSettings = { effects: true, particles: true, deepProvider: "codex", codexModel: "gpt-6.1-terra", claudeModel: "sonnet", warnDeep: true, maxNodes: 1500, depth: 2 };

/** The workspace graph: one indexed project and one folder being written now, its impact path lit. */
const DEMO_OVERALL: GraphView = {
  scope: "overall",
  grafly: { installed: true, version: "0.4.0" },
  indexed: true,
  truncated: false,
  deep: { at: "2026-01-05T09:30:00.000Z", provider: "codex", model: "gpt-6.1-terra" },
  nodes: [
    { id: "o:root", label: "전체", kind: "root", size: 40 },
    { id: "p:C:/projects/project_O", label: "project_O", kind: "project", project: "C:/projects/project_O", size: 30, indexed: true },
    { id: "p:D:/work/landing", label: "landing", kind: "project", project: "D:/work/landing", size: 10, glow: "active", indexed: false },
    { id: "a:C:/projects/project_O|app", label: "app", kind: "area", project: "C:/projects/project_O", path: "app", size: 12 },
    { id: "a:C:/projects/project_O|crates/o-core", label: "crates/o-core", kind: "area", project: "C:/projects/project_O", path: "crates/o-core", size: 10 },
    { id: "a:C:/projects/project_O|crates/o-cli", label: "crates/o-cli", kind: "area", project: "C:/projects/project_O", path: "crates/o-cli", size: 5 },
    { id: "a:D:/work/landing|src", label: "src", kind: "area", project: "D:/work/landing", path: "src", size: 8, glow: "path" },
    { id: "a:D:/work/landing|public", label: "public", kind: "area", project: "D:/work/landing", path: "public", size: 5 },
  ],
  links: [
    { source: "o:root", target: "p:C:/projects/project_O", kind: "contains", weight: 1 },
    { source: "o:root", target: "p:D:/work/landing", kind: "contains", weight: 1, glow: true },
    { source: "p:C:/projects/project_O", target: "a:C:/projects/project_O|app", kind: "contains", weight: 1 },
    { source: "p:C:/projects/project_O", target: "a:C:/projects/project_O|crates/o-core", kind: "contains", weight: 1 },
    { source: "p:C:/projects/project_O", target: "a:C:/projects/project_O|crates/o-cli", kind: "contains", weight: 1 },
    { source: "p:D:/work/landing", target: "a:D:/work/landing|src", kind: "contains", weight: 1, glow: true },
    { source: "p:D:/work/landing", target: "a:D:/work/landing|public", kind: "contains", weight: 1 },
  ],
};

/** A file node in the preview's project graph; `path` is relative to the project folder. */
const demoFile = (path: string, size: number, extra?: Partial<GraphNode>): GraphNode => ({
  id: `f:${path}`,
  label: path.split("/").pop() ?? path,
  kind: "file",
  path,
  size,
  ...extra,
});

/**
 * One project's graph (ADR-0033): three folders hold the files and calls/imports wire them.
 * `src/graph.ts` is the file changing on disk now; the two files it touches glow along the path.
 */
function demoProjectGraph(project: string): GraphView {
  const nodes: GraphNode[] = [
    { id: `p:${project}`, label: project.split("/").filter(Boolean).pop() ?? project, kind: "project", project, size: 105, indexed: true },
    { id: "d:src", label: "src", kind: "area", path: "src", size: 40, summary: "화면과 플랫폼 연결 코드가 모인 폴더" },
    { id: "d:src/core", label: "src/core", kind: "area", path: "src/core", size: 30, summary: "O CLI의 공용 규칙과 정책을 담는 핵심 폴더" },
    { id: "d:app", label: "app", kind: "area", path: "app", size: 35 },
    demoFile("src/graph.ts", 20, { glow: "active", summary: "그래프 탭 데이터를 모으는 진입점" }),
    demoFile("src/graphStyle.ts", 14, { glow: "path" }),
    demoFile("src/useGraphView.ts", 9, { glow: "path" }),
    demoFile("src/main.ts", 6),
    demoFile("src/router.ts", 4),
    demoFile("src/platform/backend.ts", 12),
    demoFile("src/platform/types.ts", 8),
    demoFile("src/platform/tauri.ts", 7),
    demoFile("src/platform/dev.ts", 5),
    demoFile("src/platform/demo.ts", 11),
    demoFile("src/core/lead.ts", 18),
    demoFile("src/core/projects.ts", 9),
    demoFile("src/core/policy.ts", 13),
    demoFile("src/core/tokens.ts", 6),
    demoFile("src/core/budget.ts", 8),
    demoFile("src/core/schedule.ts", 5),
    demoFile("src/core/review.ts", 7),
    demoFile("src/core/issues.ts", 6),
    demoFile("src/core/guard.ts", 15),
    demoFile("src/core/rules.ts", 10),
    demoFile("app/App.tsx", 14),
    demoFile("app/Composer.tsx", 12),
    demoFile("app/LeadThread.tsx", 10),
    demoFile("app/Settings.tsx", 8),
    demoFile("app/Graph3D.tsx", 16),
    demoFile("app/Office.tsx", 9),
    demoFile("app/Person.tsx", 5),
    demoFile("app/Runs.tsx", 11),
    demoFile("app/Cards.tsx", 7),
    demoFile("app/Chat.tsx", 13),
  ];
  const folderOf = (path: string) => (path.startsWith("src/core/") ? "d:src/core" : path.startsWith("src/") ? "d:src" : "d:app");
  const contains: GraphLink[] = [
    { source: `p:${project}`, target: "d:src", kind: "contains", weight: 1 },
    { source: `p:${project}`, target: "d:app", kind: "contains", weight: 1 },
    { source: "d:src", target: "d:src/core", kind: "contains", weight: 1 },
    ...nodes.filter((n) => n.kind === "file").map((n) => ({ source: folderOf(n.path ?? ""), target: n.id, kind: "contains" as const, weight: 1 })),
  ];
  const wires: GraphLink[] = [
    { source: "f:src/graph.ts", target: "f:src/graphStyle.ts", kind: "calls", weight: 3, glow: true },
    { source: "f:src/graph.ts", target: "f:src/useGraphView.ts", kind: "calls", weight: 2, glow: true },
    { source: "f:src/graph.ts", target: "f:src/platform/backend.ts", kind: "calls", weight: 2 },
    { source: "f:src/graphStyle.ts", target: "f:src/platform/types.ts", kind: "imports", weight: 2 },
    { source: "f:src/useGraphView.ts", target: "f:src/platform/backend.ts", kind: "calls", weight: 3 },
    { source: "f:src/main.ts", target: "f:src/router.ts", kind: "imports", weight: 2 },
    { source: "f:src/router.ts", target: "f:app/App.tsx", kind: "imports", weight: 1 },
    { source: "f:app/App.tsx", target: "f:app/Chat.tsx", kind: "imports", weight: 2 },
    { source: "f:app/Chat.tsx", target: "f:app/Composer.tsx", kind: "imports", weight: 2 },
    { source: "f:app/Chat.tsx", target: "f:app/LeadThread.tsx", kind: "imports", weight: 1 },
    { source: "f:app/Graph3D.tsx", target: "f:src/graphStyle.ts", kind: "imports", weight: 3 },
    { source: "f:app/Graph3D.tsx", target: "f:src/useGraphView.ts", kind: "calls", weight: 1 },
    { source: "f:app/Office.tsx", target: "f:app/Person.tsx", kind: "imports", weight: 2 },
    { source: "f:app/Runs.tsx", target: "f:app/Cards.tsx", kind: "imports", weight: 2 },
    { source: "f:app/Settings.tsx", target: "f:src/platform/backend.ts", kind: "calls", weight: 1 },
    { source: "f:src/platform/backend.ts", target: "f:src/platform/types.ts", kind: "imports", weight: 4 },
    { source: "f:src/platform/tauri.ts", target: "f:src/platform/types.ts", kind: "imports", weight: 3 },
    { source: "f:src/platform/dev.ts", target: "f:src/platform/types.ts", kind: "imports", weight: 2 },
    { source: "f:src/platform/demo.ts", target: "f:src/platform/types.ts", kind: "imports", weight: 2 },
    { source: "f:src/platform/backend.ts", target: "f:src/platform/tauri.ts", kind: "calls", weight: 1 },
    { source: "f:src/core/lead.ts", target: "f:src/core/policy.ts", kind: "calls", weight: 3 },
    { source: "f:src/core/lead.ts", target: "f:src/core/projects.ts", kind: "calls", weight: 2 },
    { source: "f:src/core/tokens.ts", target: "f:src/core/budget.ts", kind: "calls", weight: 2 },
    { source: "f:src/core/schedule.ts", target: "f:src/core/lead.ts", kind: "calls", weight: 2 },
    { source: "f:src/core/guard.ts", target: "f:src/core/rules.ts", kind: "calls", weight: 4 },
  ];
  return {
    scope: "project",
    project,
    grafly: { installed: true, version: "0.4.0" },
    indexed: true,
    truncated: false,
    deep: { at: "2026-01-05T09:30:00.000Z", provider: "codex", model: "gpt-6.1-terra" },
    nodes,
    links: [...contains, ...wires],
  };
}

/** The recorded demo: every agent installed and signed in, nothing really runs. */
export const demoBackend: Backend = {
  kind: "demo",
  resources: () => Promise.resolve([]),
  recoverResource: () => Promise.reject(new Error("미리보기에서는 예약을 복구하지 않습니다")),
  imageRead: async () => { throw new Error("미리보기에서는 로컬 이미지를 열 수 없습니다"); },
  imageOpen: async () => { throw new Error("미리보기에서는 로컬 이미지를 열 수 없습니다"); },
  schedules: async () => [],
  scheduleCreate: async () => { throw new Error("미리보기에서는 예약을 만들 수 없습니다"); },
  scheduleDelete: async () => { throw new Error("미리보기에서는 예약을 삭제할 수 없습니다"); },
  scheduleSetEnabled: async () => { throw new Error("미리보기에서는 예약을 바꿀 수 없습니다"); },
  projectOpen: async () => { throw new Error("미리보기에서는 로컬 폴더를 열 수 없습니다"); },
  agentsCatalog: async () => [],
  agents: async () => [
    { id: "claude", name: "Claude Code", installed: true, version: "demo", loggedIn: true, installHint: "", canInstall: false, installUrl: "", apiKeyEnv: [] },
    { id: "codex", name: "Codex", installed: true, version: "demo", loggedIn: true, installHint: "", canInstall: false, installUrl: "", apiKeyEnv: [] },
    { id: "agy", name: "Antigravity", installed: true, version: "demo", loggedIn: true, installHint: "", canInstall: false, installUrl: "", apiKeyEnv: [] },
  ],
  install: async () => ({ ok: false, message: "미리보기에서는 설치하지 않습니다" }),
  login: async () => ({ ok: false, message: "미리보기에서는 연결하지 않습니다" }),
  setup: async () => ({ ok: false, message: "미리보기에서는 에이전트를 설정하거나 점검하지 않습니다" }),
  openTerminal: async () => ({ ok: false, message: "미리보기에서는 터미널을 열 수 없습니다" }),
  openExternal: async (url) => void window.open(url, "_blank", "noopener"),
  about: async () => ({ version: "preview", home: "", agentsHome: "", commit: "", builtAt: "" }),
  checkUpdate: async () => ({ release: null, message: DESKTOP_UPDATES_ONLY }),
  pendingUpdate: async () => null,
  installUpdate: async () => { throw new Error(DESKTOP_UPDATES_ONLY); },
  cancelUpdate: async () => {},
  updateStatus: async () => NO_UPDATE_STATUS,
  review: async () => ({ ok: false, message: "미리보기에서는 검토를 요청하지 않습니다" }),
  issuesSearch: async () => { throw new Error("미리보기에서는 이슈를 조회할 수 없습니다"); },
  issuesList: async () => [],
  changes: async () => { throw new Error("미리보기에서는 작업 폴더 변경을 조회할 수 없습니다"); },
  issuesRecord: async () => { throw new Error("미리보기에서는 발견을 저장할 수 없습니다"); },
  issuesClassify: async () => { throw new Error("미리보기에서는 분류를 저장할 수 없습니다"); },
  issuesResolve: async () => { throw new Error("미리보기에서는 검증 결과를 저장할 수 없습니다"); },
  budget: async () => demoBudget(demoPercent, demoEnabled),
  setBudgetPercent: async (percent) => demoBudget((demoPercent = percent), demoEnabled),
  setBudgetEnabled: async (enabled) => demoBudget(demoPercent, (demoEnabled = enabled)),
  ram: async () => ({ ...demoRam }),
  setRam: async (change) => ({ ...Object.assign(demoRam, change) }),
  billing: async () => ({ ...demoBilling }),
  setBilling: async (mode) => ({ ...Object.assign(demoBilling, { mode }) }),
  yolo: async () => ({ enabled: false, budgetEnabled: true, autoRun: { maxCards: 3, perHour: 2, dedupeMinutes: 30 } }),
  setYolo: async () => { throw new Error("미리보기에서는 YOLO 설정을 바꾸지 않습니다"); },
  tokens: async () => ({
    groups: [DEMO_BASELINE, DEMO_SHARED],
    compare: { baseline: DEMO_BASELINE, shared: DEMO_SHARED },
    summary: {
      reported: demoTokens(227_700, 486_000, 52_200),
      reportedCards: 18,
      unreportedCards: 0,
      incompleteCards: 0,
      cachePercent: 486_000 / (227_700 + 486_000) * 100,
    },
  }),
  // The recorded preview shows the demo's own transcripts and fake open tabs; nothing reaches a lead.
  leadShow: async (project, thread) => {
    if (!project) return { thread: null, projects: demoLeadProjects() };
    const tabs = demoOpen(project);
    const tab = tabs.find((t) => t.threadId === thread) ?? tabs[0];
    return {
      thread: tab ? { threadId: tab.threadId, project, busy: tab.busy, events: [{ t: "lead.start" as const, at: tab.startedAt, threadId: tab.threadId, project }] } : null,
      threads: tabs,
      projects: demoLeadProjects(),
    };
  },
  leadNote: async () => { throw new Error(LEAD_REFUSED); },
  leadNew: async (project) => {
    const tabs = demoOpen(project);
    if (tabs.length >= LEAD_TAB_MAX) throw new Error("열린 총괄은 최대 4개입니다");
    const tab: LeadTab = { threadId: `${new Date().toISOString().replace(/\D/g, "").slice(0, 14)}-new${demoTabSeq++}`, title: "", state: "idle", busy: null, writer: !tabs.some((t) => t.writer), startedAt: new Date().toISOString() };
    tabs.push(tab);
    return { threadId: tab.threadId };
  },
  leadClose: async (threadId) => {
    for (const tabs of demoTabs.values()) {
      const at = tabs.findIndex((t) => t.threadId === threadId);
      if (at >= 0) { tabs.splice(at, 1); return; }
    }
    throw new Error("열린 총괄이 아닙니다");
  },
  leadWriter: async (threadId) => {
    for (const tabs of demoTabs.values()) {
      if (tabs.some((t) => t.threadId === threadId)) {
        for (const t of tabs) t.writer = t.threadId === threadId;
        return;
      }
    }
    throw new Error("열린 총괄이 아닙니다");
  },
  followup: () => Promise.reject(new Error(LEAD_REFUSED)),
  reply: () => Promise.reject(new Error(LEAD_REFUSED)),
  attention: async () => {},
  leadAttach: () => Promise.reject(new Error(LEAD_REFUSED)),
  leadSend: () => Promise.reject(new Error(LEAD_REFUSED)),
  leadDispatch: () => Promise.reject(new Error(LEAD_REFUSED)),
  leadForward: () => Promise.reject(new Error(LEAD_REFUSED)),
  leadDecide: () => Promise.reject(new Error(LEAD_REFUSED)),
  leadStop: async () => ({ stopped: false }),
  leadWake: async () => { throw new Error("미리보기에서는 총괄을 실행할 수 없습니다"); },
  leadRetry: async () => { throw new Error("미리보기에서는 메시지를 복구할 수 없습니다"); },
  leadAnswerApproval: async () => { throw new Error("미리보기에서는 답변을 보낼 수 없습니다"); },
  leadStopAll: async () => ({ stopped: 0 }),
  leadModels: async (provider) => ({ provider, models: DEMO_MODELS[provider] }),
  graphView: async (project) => (project ? demoProjectGraph(project) : DEMO_OVERALL),
  graphBuild: async (project) => ({ ok: true, output: `미리보기용 인덱스입니다: ${project}` }),
  graphDeep: async (project, provider, model): Promise<GraphDeepResult> => ({
    areas: [
      { id: "d:src", name: "src", summary: "화면과 플랫폼 연결 코드가 모인 폴더" },
      { id: "d:src/core", name: "src/core", summary: "O CLI의 공용 규칙과 정책을 담는 핵심 폴더" },
      { id: "d:app", name: "app", summary: "Tauri 셸과 React 화면 코드" },
    ],
    provider: provider ?? "codex",
    model: model ?? "gpt-6.1-terra",
    at: new Date().toISOString(),
    usage: { input: 42100, output: 980 },
  }),
  graphSettings: async (change) => (demoGraphSettings = { ...demoGraphSettings, ...change }),
  projects: async () => demoList(),
  projectsAdd: async (path) => {
    if (!ABSOLUTE.test(path)) throw new Error("project는 절대 경로여야 합니다");
    if (demoProjects.some((p) => sameFolder(p.path, path))) throw new Error(`이미 목록에 있는 폴더입니다: ${path}`);
    demoProjects = [...demoProjects, { path: path.replace(/\\/g, "/"), name: nameOf(path), kind: "registered" }];
    return demoList();
  },
  projectsRemove: async (path) => {
    const found = demoProjects.find((p) => sameFolder(p.path, path));
    if (!found) throw new Error(`등록된 폴더가 아닙니다: ${path}`);
    if (found.kind !== "registered") throw new Error("프로젝트 루트 안의 폴더는 목록에서 뺄 수 없습니다");
    demoProjects = demoProjects.filter((p) => p !== found);
    return demoList();
  },
  projectsNew: async (name) => {
    const path = `${DEMO_ROOT}/${name.trim()}`;
    if (!name.trim() || /[\\/:*?"<>|]/.test(name)) throw new Error("프로젝트 이름에는 글자·숫자·공백·. _ -만 쓸 수 있습니다");
    if (demoProjects.some((p) => sameFolder(p.path, path))) throw new Error(`같은 이름의 폴더가 이미 있습니다: ${path}`);
    demoProjects = [...demoProjects, { path, name: name.trim(), kind: "root" }];
    return { ...demoList(), created: path };
  },
  projectsRoot: async () => demoList(),
  projectsCheck: async (paths) => paths.map((path) => (ABSOLUTE.test(path) ? { path, ok: true } : { path, ok: false, reason: "project는 절대 경로여야 합니다" })),
};
