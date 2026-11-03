import type { AgentId, Backend, BudgetReport, BudgetState, LeadModel, LeadProvider, ListedProject, ProjectList, ProviderBudget, TokenCounts, TokenGroup } from "./types.ts";
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

/** The recorded demo: every agent installed and signed in, nothing really runs. */
export const demoBackend: Backend = {
  kind: "demo",
  imageRead: async () => { throw new Error("미리보기에서는 로컬 이미지를 열 수 없습니다"); },
  imageOpen: async () => { throw new Error("미리보기에서는 로컬 이미지를 열 수 없습니다"); },
  schedules: async () => [],
  scheduleCreate: async () => { throw new Error("미리보기에서는 예약을 만들 수 없습니다"); },
  scheduleDelete: async () => { throw new Error("미리보기에서는 예약을 삭제할 수 없습니다"); },
  scheduleSetEnabled: async () => { throw new Error("미리보기에서는 예약을 바꿀 수 없습니다"); },
  projectOpen: async () => { throw new Error("미리보기에서는 로컬 폴더를 열 수 없습니다"); },
  agentsCatalog: async () => [],
  agents: async () => [
    { id: "claude", name: "Claude Code", installed: true, version: "demo", loggedIn: true, installHint: "", canInstall: false, installUrl: "" },
    { id: "codex", name: "Codex", installed: true, version: "demo", loggedIn: true, installHint: "", canInstall: false, installUrl: "" },
    { id: "agy", name: "Antigravity", installed: true, version: "demo", loggedIn: true, installHint: "", canInstall: false, installUrl: "" },
  ],
  install: async () => ({ ok: false, message: "미리보기에서는 설치하지 않습니다" }),
  login: async () => ({ ok: false, message: "미리보기에서는 연결하지 않습니다" }),
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
  issuesRecord: async () => { throw new Error("미리보기에서는 발견을 저장할 수 없습니다"); },
  issuesClassify: async () => { throw new Error("미리보기에서는 분류를 저장할 수 없습니다"); },
  issuesResolve: async () => { throw new Error("미리보기에서는 검증 결과를 저장할 수 없습니다"); },
  budget: async () => demoBudget(demoPercent, demoEnabled),
  setBudgetPercent: async (percent) => demoBudget((demoPercent = percent), demoEnabled),
  setBudgetEnabled: async (enabled) => demoBudget(demoPercent, (demoEnabled = enabled)),
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
  // The recorded preview shows the demo's own transcripts; nothing reaches a lead.
  leadShow: async () => ({ thread: null, projects: [] }),
  leadNote: async () => { throw new Error(LEAD_REFUSED); },
  leadNew: () => Promise.reject(new Error(LEAD_REFUSED)),
  followup: () => Promise.reject(new Error(LEAD_REFUSED)),
  leadAttach: () => Promise.reject(new Error(LEAD_REFUSED)),
  leadSend: () => Promise.reject(new Error(LEAD_REFUSED)),
  leadDispatch: () => Promise.reject(new Error(LEAD_REFUSED)),
  leadStop: async () => ({ stopped: false }),
  leadWake: async () => { throw new Error("미리보기에서는 총괄을 실행할 수 없습니다"); },
  leadRetry: async () => { throw new Error("미리보기에서는 메시지를 복구할 수 없습니다"); },
  leadStopAll: async () => ({ stopped: 0 }),
  leadModels: async (provider) => ({ provider, models: DEMO_MODELS[provider] }),
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
