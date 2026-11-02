import type { AgentId, Backend, BudgetReport, BudgetState, LeadModel, LeadProvider, ProviderBudget, TokenCounts, TokenGroup } from "./types.ts";
import { DESKTOP_UPDATES_ONLY, NO_UPDATE_STATUS } from "../updates.ts";

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

/** The preview's model lists, in the shape `o lead models` prints (Claude's are its documented aliases). */
const DEMO_MODELS: Record<LeadProvider, LeadModel[]> = {
  claude: [
    { id: "fable", label: "Fable" },
    { id: "opus", label: "Opus" },
    { id: "sonnet", label: "Sonnet" },
    { id: "haiku", label: "Haiku" },
  ],
  codex: [
    { id: "gpt-6.1-sol", label: "GPT-6.1-Sol" },
    { id: "gpt-6.1-terra", label: "GPT-6.1 Terra" },
  ],
  agy: [
    { id: "gemini-4-argon-high", label: "Gemini 4 Argon (High)" },
    { id: "gemini-3.8-flash-medium", label: "Gemini 3.8 Flash (Medium)" },
  ],
};

/** The recorded demo: every agent installed and signed in, nothing really runs. */
export const demoBackend: Backend = {
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
  updateStatus: async () => NO_UPDATE_STATUS,
  review: async () => ({ ok: false, message: "미리보기에서는 검토를 요청하지 않습니다" }),
  budget: async () => demoBudget(demoPercent),
  setBudgetPercent: async (percent) => demoBudget((demoPercent = percent)),
  tokens: async () => ({ groups: [DEMO_BASELINE, DEMO_SHARED], compare: { baseline: DEMO_BASELINE, shared: DEMO_SHARED } }),
  // The recorded preview shows the demo's own transcripts; nothing reaches a lead.
  leadShow: async () => ({ thread: null, projects: [] }),
  leadNew: () => Promise.reject(new Error(LEAD_REFUSED)),
  leadAttach: () => Promise.reject(new Error(LEAD_REFUSED)),
  leadSend: () => Promise.reject(new Error(LEAD_REFUSED)),
  leadDispatch: () => Promise.reject(new Error(LEAD_REFUSED)),
  leadStop: async () => ({ stopped: false }),
  leadModels: async (provider) => ({ provider, models: DEMO_MODELS[provider] }),
};
