import type { AgentId, AgyReport, Backend, BillingReport, BudgetReport, BudgetState, GraphDeepResult, GraphLink, GraphNode, GraphSettings, GraphView, LeadModel, LeadProvider, ListedProject, ProjectList, ProviderBudget, RamReport, TermChunk, TermSession, TokenCounts, TokenGroup, DreamDoc, DreamChange, DreamGraph, DreamPage } from "./types.ts";
import { LEAD_TAB_MAX, type LeadShow, type LeadTab } from "../../../../src/core/lead.ts";
import { folderKey, sameFolder } from "../../../../src/core/projects.ts";
import { agyApply, b64ToBytes, bytesToB64, type AgyState } from "../../../../src/core/terminal.ts";
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

/** The preview's Antigravity consent (ADR-0042): the policy text the contract fixes, and the same ack rule `o agy` applies. */
const AGY_NOTICE: AgyReport["notice"] = {
  quotes: [
    "Using third-party software, tools, or services to access Antigravity is a violation of our Terms of Service and severely degrades the experience for legitimate product users.",
    "Such actions can result in suspension or termination of your account.",
    "Wrapping `agy` inside third-party coding agents or applications, or automating account rotation, is not supported under consumer plans.",
  ],
  links: [
    { label: "Antigravity FAQ", url: "https://antigravity.google/docs/faq" },
    { label: "Antigravity 약관", url: "https://antigravity.google/terms" },
  ],
  warning: "O가 agy 터미널에 글을 자동으로 넣는 것은 제3자 앱이 agy를 다루는 것으로 볼 수 있습니다. 계정 정지·해지의 위험은 이 PC에 로그인한 Google 계정이 집니다. 기업 계약은 조건이 다를 수 있습니다.",
};
let demoAgy: AgyState = { workers: false, acknowledgedAt: null, confirmPaste: true };
const agyReport = (): AgyReport => ({ ...demoAgy, notice: AGY_NOTICE });

/** A fake terminal session for the preview: it prints a short banner and echoes what is typed. */
interface DemoTerm {
  session: TermSession;
  bytes: number[];
}
const demoTerms = new Map<string, DemoTerm>();
let demoTermSeq = 0;
const UTF8 = new TextEncoder();
const demoTerm = (id: string): DemoTerm | undefined => demoTerms.get(id);

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

/** The preview's Dreaming documents (ADR-0041): two materials and a few card records, and what wires them. */
interface DemoDoc extends DreamDoc {
  text: string;
  source?: string;
  links: string[];
}

const demoAgo = (hours: number) => new Date(Date.now() - hours * 3_600_000).toISOString();
const DEMO_BLOG = "C:/projects/blog";

let demoDocs: DemoDoc[] = [
  { path: "외부 자료/20261108-101500-tauri-2-배포-안내.md", title: "Tauri 2 배포 안내", kind: "external", project: DEMO_BLOG, at: demoAgo(50), size: 310, tags: ["배포", "tauri"], source: "https://example.invalid/tauri/distribute", text: "## 배포 순서\n\n- 서명 키를 준비합니다.\n- `tauri build`로 설치 파일을 만듭니다.\n\n더 볼 것: [[릴리스 점검표]]\n", links: ["외부 자료/20261107-090000-릴리스-점검표.md"] },
  { path: "외부 자료/20261107-090000-릴리스-점검표.md", title: "릴리스 점검표", kind: "external", at: demoAgo(74), size: 180, tags: ["배포", "점검"], source: "release-checklist.md", text: "# 릴리스 점검표\n\n1. 테스트 통과\n2. 라이선스 검사\n3. 프리릴리스 확인\n", links: [] },
  { path: "작업 성공/blog/20261110-091200-k3fa-build.md", title: "설치 파일 만들기 · 빌드", kind: "done", project: DEMO_BLOG, at: demoAgo(3), size: 240, tags: ["서명", "tauri"], text: "# 설치 파일 만들기 · 빌드\n\n- **결과**: 성공\n- **프로젝트**: `C:/projects/blog`\n\n## 맡긴 일\n\n설치 파일을 만들고 크기를 확인합니다.\n\n## 결과\n\nhttps://example.invalid/tauri/distribute 순서대로 설치 파일을 만들었습니다.\n\n## 바뀐 파일\n\n- `scripts/build-installer.ps1`\n", links: ["외부 자료/20261108-101500-tauri-2-배포-안내.md", "작업 실패/blog/20261109-173000-p8zq-build.md"] },
  { path: "작업 성공/blog/20261110-091200-k3fa-test.md", title: "설치 파일 만들기 · 검증", kind: "done", project: DEMO_BLOG, at: demoAgo(2), size: 120, text: "# 설치 파일 만들기 · 검증\n\n- **결과**: 성공\n\n## 결과\n\n설치와 제거를 확인했습니다.\n", links: ["작업 성공/blog/20261110-091200-k3fa-build.md"] },
  { path: "작업 실패/blog/20261109-173000-p8zq-build.md", title: "설치 파일 만들기 · 빌드", kind: "failed", project: DEMO_BLOG, at: demoAgo(19), size: 200, text: "# 설치 파일 만들기 · 빌드\n\n- **결과**: 실패 (종료 코드 1)\n- **프로젝트**: `C:/projects/blog`\n\n## 결과\n\n서명 키가 없어 `tauri build`가 멈췄습니다.\n\n## 바뀐 파일\n\n- `scripts/build-installer.ps1`\n", links: [] },
  { path: "배운 것/blog/20261110-091200-k3fa-build-1.md", title: "서명 키는 환경 변수로만 읽는다", kind: "note", project: DEMO_BLOG, at: demoAgo(3), size: 120, tags: ["서명", "tauri"], source: "설치 파일 만들기 · 빌드", text: "# 서명 키는 환경 변수로만 읽는다\n\n`tauri build` 전에 서명 키 환경 변수가 있어야 합니다. 없으면 설치 파일 단계에서 멈춥니다. #서명 #tauri\n", links: ["작업 성공/blog/20261110-091200-k3fa-build.md"] },
  { path: "도구 오류/blog/20261109-173000-p8zq-build.md", title: "도구 오류 3건 · 설치 파일 만들기 · 빌드", kind: "tool", project: DEMO_BLOG, at: demoAgo(19), size: 3, text: "# 도구 오류 3건 · 설치 파일 만들기 · 빌드\n\n- **프로젝트**: `C:/projects/blog`\n- **실패한 호출**: 3건 (서로 다른 호출 2개)\n\n## 실패한 호출\n\n- **Bash** `npm run tauri biuld` — 2번\n  - Missing script: biuld\n- **Read** `src-tauri/tauri.conf.json5`\n  - File does not exist.\n", links: ["작업 실패/blog/20261109-173000-p8zq-build.md"] },
  { path: "작업 성공/landing/20261106-140000-w2ab-copy.md", title: "첫 화면 문구 · 작성", kind: "done", project: "C:/projects/landing", at: demoAgo(96), size: 90, text: "# 첫 화면 문구 · 작성\n\n- **결과**: 성공\n\n## 결과\n\n문구 세 가지를 제안했습니다.\n", links: [] },
];

let demoChanges: DreamChange[] = [
  { id: "demo-tool-1", kind: "tool-failure", project: DEMO_BLOG, text: "`Bash npm run tauri biuld` 호출이 실패했습니다: Missing script: biuld (3번)", count: 3, firstAt: demoAgo(43), lastAt: demoAgo(19), state: "applied", changedAt: demoAgo(19), evidence: [{ runId: "20261108-173000-a1bc", cardId: "build" }, { runId: "20261109-173000-p8zq", cardId: "build" }] },
  { id: "demo-card-1", kind: "card-failure", project: DEMO_BLOG, text: "카드가 같은 이유로 실패했습니다: 서명 키가 없어 tauri build가 멈췄습니다 (2번)", count: 2, firstAt: demoAgo(43), lastAt: demoAgo(19), state: "applied", changedAt: demoAgo(19), evidence: [{ runId: "20261109-173000-p8zq", cardId: "build" }] },
];

/** The Dreaming graph of the preview's documents: today's records glow with what they are wired to. */
function demoDreamGraph(): DreamGraph {
  const fresh = (d: DemoDoc) => Date.now() - Date.parse(d.at) < 24 * 3_600_000;
  const glowOf = (d: DemoDoc) => (fresh(d) ? "active" : demoDocs.some((other) => fresh(other) && other.links.includes(d.path)) ? "path" : undefined);
  const folders = [...new Set(demoDocs.flatMap((d) => d.path.split("/").slice(0, -1).map((_, i, parts) => parts.slice(0, i + 1).join("/"))))];
  const under = (dir: string) => demoDocs.filter((d) => d.path.startsWith(`${dir}/`));
  const parentOf = (path: string) => (path.includes("/") ? `d:${path.slice(0, path.lastIndexOf("/"))}` : "dream");
  const nodes: GraphNode[] = [
    { id: "dream", label: "Dreaming", kind: "root", size: demoDocs.length, glow: demoDocs.some(fresh) ? "active" : undefined },
    ...folders.map((dir): GraphNode => ({ id: `d:${dir}`, label: dir.split("/").pop() ?? dir, kind: "area", path: dir, size: under(dir).length, glow: under(dir).some(fresh) ? "active" : under(dir).some(glowOf) ? "path" : undefined })),
    ...demoDocs.map((d): GraphNode => ({ id: `f:${d.path}`, label: d.title, kind: "file", project: d.project, path: d.path, size: 1 + demoDocs.filter((other) => other.links.includes(d.path)).length, glow: glowOf(d), summary: d.source })),
  ];
  const links: GraphLink[] = [
    ...folders.map((dir): GraphLink => ({ source: parentOf(dir), target: `d:${dir}`, kind: "contains", weight: 1, glow: under(dir).some(glowOf) || undefined })),
    ...demoDocs.map((d): GraphLink => ({ source: parentOf(d.path), target: `f:${d.path}`, kind: "contains", weight: 1, glow: glowOf(d) !== undefined || undefined })),
    ...demoDocs.flatMap((d) => d.links.filter((to) => demoDocs.some((other) => other.path === to)).map((to): GraphLink => ({ source: `f:${d.path}`, target: `f:${to}`, kind: "references", weight: 1, glow: fresh(d) || undefined }))),
  ];
  return { nodes, links, truncated: false };
}

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
  canDriveTerminals: true,
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
  about: async () => ({ version: "preview", home: "%LOCALAPPDATA%\\project_O", agentsHome: "%LOCALAPPDATA%\\project_O-agents", commit: "", builtAt: "" }),
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
  agyGet: async () => agyReport(),
  agySet: async (change) => {
    const next = agyApply(demoAgy, change, new Date().toISOString());
    if (typeof next === "string") throw new Error(next);
    demoAgy = next;
    return agyReport();
  },
  termOpen: async (request) => {
    const session: TermSession = {
      id: `t-demo-${String(++demoTermSeq).padStart(4, "0")}`,
      provider: request.provider,
      project: request.project,
      purpose: "lead",
      threadId: request.thread,
      state: "running",
      exitCode: null,
      cols: request.cols ?? 120,
      rows: request.rows ?? 32,
      startedAt: new Date().toISOString(),
    };
    const banner = `\x1b[1m${request.provider} (미리보기)\x1b[0m\r\n미리보기 터미널입니다. 입력한 글자가 그대로 돌아옵니다.\r\n$ `;
    demoTerms.set(session.id, { session, bytes: [...UTF8.encode(banner)] });
    return session;
  },
  termList: async () => [...demoTerms.values()].map((t) => t.session),
  termRead: async (id, offset): Promise<TermChunk> => {
    const term = demoTerm(id);
    if (!term) throw new Error("없는 터미널 세션입니다");
    const from = Math.min(offset, term.bytes.length);
    return { offset: term.bytes.length, data: bytesToB64(Uint8Array.from(term.bytes.slice(from))), state: term.session.state, exitCode: term.session.exitCode, start: 0 };
  },
  termWrite: async (id, dataBase64) => {
    const term = demoTerm(id);
    if (!term || term.session.state === "exited") return;
    for (const byte of b64ToBytes(dataBase64)) {
      if (byte === 13) term.bytes.push(13, 10, ...UTF8.encode("$ "));
      else term.bytes.push(byte);
    }
  },
  termResize: async (id, cols, rows) => {
    const term = demoTerm(id);
    if (term) Object.assign(term.session, { cols, rows });
  },
  termPaste: async (id) => {
    const term = demoTerm(id);
    if (term?.session.state === "awaiting-paste") term.session.state = "running";
  },
  termClose: async (id) => {
    demoTerms.delete(id);
  },
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
  dreamList: async () => ({ docs: demoDocs.map(({ path, title, kind, project, at, size, tags }) => ({ path, title, kind, project, at, size, tags })), truncated: false }),
  dreamRead: async (path): Promise<DreamPage> => {
    const doc = demoDocs.find((d) => d.path === path);
    if (!doc) throw new Error(`목록에 없는 문서입니다: ${path}`);
    const { size: _size, links, ...page } = doc;
    return { ...page, truncated: false, links: links.filter((to) => demoDocs.some((d) => d.path === to)), backlinks: demoDocs.filter((d) => d.links.includes(path)).map((d) => d.path) };
  },
  dreamGraph: async () => demoDreamGraph(),
  dreamAdd: async (input) => {
    // The preview fetches nothing: a web address is kept as a note of where the material would come from.
    const text = input.kind === "text" ? input.text.trim() : `미리보기에서는 주소를 읽어 오지 않습니다: ${input.url}`;
    if (!text) throw new Error("넣을 내용이 비어 있습니다");
    const title = input.title?.trim() || (input.kind === "url" ? input.url : (text.split("\n").find((l) => l.trim()) ?? "자료").replace(/^#\s*/, "")).slice(0, 120);
    const doc: DemoDoc = { path: `외부 자료/${Date.now()}-${title.replace(/[^\p{L}\p{N}]+/gu, "-").slice(0, 40)}.md`, title, kind: "external", project: input.project, at: new Date().toISOString(), size: text.length, tags: [...new Set((input.tags ?? "").toLowerCase().split(/[\s,]+/).map((tag) => tag.replace(/^#+/, "")).filter(Boolean))], source: input.kind === "url" ? input.url : input.source, text, links: [] };
    demoDocs = [doc, ...demoDocs];
    const { text: _text, source: _source, links: _links, ...listed } = doc;
    return listed;
  },
  dreamRemove: async (path) => {
    if (!demoDocs.some((d) => d.path === path && d.kind === "external")) throw new Error("외부 자료만 뺄 수 있습니다");
    demoDocs = demoDocs.filter((d) => d.path !== path);
  },
  dreamChanges: async () => demoChanges,
  dreamRevert: async (id, reverted) => {
    const change = demoChanges.find((c) => c.id === id);
    if (!change) throw new Error(`없는 변경입니다: ${id}`);
    const next: DreamChange = { ...change, state: reverted ? "reverted" : "applied", changedAt: new Date().toISOString() };
    demoChanges = demoChanges.map((c) => (c.id === id ? next : c));
    return next;
  },
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
