import { CARD_FIELDS, PLAN_FIELDS, PROVIDERS, isProvider, type Card, type Plan, type ProviderId } from "./plan.ts";
import { CODE_RULES, findHits } from "../guard/rules.ts";

/**
 * The rules that decide whether a plan may run. The supervising agent only
 * proposes; this module decides, and a plan with any violation runs nothing.
 *
 * Limits here are ceilings, not defaults: there is deliberately no setting,
 * flag or environment variable that raises them. Changing one is an ADR.
 */
export const LIMITS = {
  maxCardsPerPlan: 8,
  maxTaskChars: 20_000,
  maxMessageChars: 4_000,
  maxActiveTotal: 6,
  maxActivePerProvider: { claude: 3, codex: 3, agy: 2 } as Record<ProviderId, number>,
  maxCardsPerDay: 60,
  /** When a provider reports a limit without a reset time. */
  defaultCooldownMs: 60 * 60 * 1000,
} as const;

export const EFFORTS = ["minimal", "low", "medium", "high", "xhigh", "max"] as const;
const MODEL = /^[A-Za-z0-9][A-Za-z0-9._:\-/[\]]{0,79}$/;
const CARD_ID = /^[a-z0-9][a-z0-9-]{0,31}$/;

export interface Violation {
  rule: string;
  message: string;
  cardId?: string;
}

/** What the machine is doing right now, read from O's state directory. */
export interface PolicyContext {
  now: Date;
  activeByProvider: Record<ProviderId, number>;
  startedToday: number;
  /** Provider → ISO time until which it may not be used. */
  cooldowns: Partial<Record<ProviderId, string>>;
  projectExists: (path: string) => boolean;
  /** Whether the folder is a project_O test/main mirror checkout. */
  isMirror: (path: string) => boolean;
}

export function emptyCounts(): Record<ProviderId, number> {
  return Object.fromEntries(PROVIDERS.map((p) => [p, 0])) as Record<ProviderId, number>;
}

export function checkPlan(input: unknown, ctx: PolicyContext): { plan?: Plan; violations: Violation[] } {
  const violations: Violation[] = [];
  const add = (rule: string, message: string, cardId?: string) => violations.push({ rule, message, cardId });

  if (!isRecord(input)) {
    add("plan-shape", "계획은 JSON 객체여야 합니다");
    return { violations };
  }
  for (const key of Object.keys(input)) {
    if (!(PLAN_FIELDS as readonly string[]).includes(key)) add("unsupported-field", `계획에 허용되지 않는 필드: ${key}`);
  }
  const { title, project, cards } = input;
  if (typeof title !== "string" || !title.trim()) add("plan-shape", "title이 필요합니다");
  if (typeof project !== "string" || !isAbsolute(project)) add("plan-shape", "project는 절대 경로여야 합니다");
  else if (!ctx.projectExists(project)) add("plan-shape", `project 폴더가 없습니다: ${project}`);
  else if (ctx.isMirror(project)) {
    add("read-only-project", `${project}는 project_O의 test·main 거울 폴더라 작업을 보낼 수 없습니다. 개인 브랜치 폴더를 지정하세요`);
  }
  if (!Array.isArray(cards) || cards.length === 0) {
    add("plan-shape", "cards가 1개 이상 필요합니다");
    return { violations };
  }
  if (cards.length > LIMITS.maxCardsPerPlan) add("plan-size", `카드는 계획당 최대 ${LIMITS.maxCardsPerPlan}개입니다`);

  const ids = new Set<string>();
  const valid: Card[] = [];
  for (const raw of cards) {
    const card = checkCard(raw, add);
    if (!card) continue;
    if (ids.has(card.id)) add("plan-shape", `카드 id가 중복됩니다: ${card.id}`, card.id);
    ids.add(card.id);
    valid.push(card);
  }

  for (const card of valid) {
    const until = ctx.cooldowns[card.provider];
    if (until && Date.parse(until) > ctx.now.getTime()) {
      add(
        "provider-cooldown",
        `${card.provider}는 사용량 한도에 걸려 ${until}까지 쓸 수 없습니다. 같은 서비스의 다른 계정으로 돌려 쓰는 것도 막혀 있습니다`,
        card.id,
      );
    }
  }

  const activeTotal = Object.values(ctx.activeByProvider).reduce((a, b) => a + b, 0);
  if (activeTotal >= LIMITS.maxActiveTotal) {
    add("active-cap", `이미 ${activeTotal}개 작업이 실행 중입니다(최대 ${LIMITS.maxActiveTotal}). 끝난 뒤 다시 보내세요`);
  }
  for (const provider of PROVIDERS) {
    const wanted = valid.some((c) => c.provider === provider);
    if (wanted && ctx.activeByProvider[provider] >= LIMITS.maxActivePerProvider[provider]) {
      add("active-cap", `${provider} 작업이 이미 최대치(${LIMITS.maxActivePerProvider[provider]})로 실행 중입니다`);
    }
  }
  if (ctx.startedToday + valid.length > LIMITS.maxCardsPerDay) {
    add("daily-cap", `오늘 실행한 카드 ${ctx.startedToday}개 — 하루 최대 ${LIMITS.maxCardsPerDay}개를 넘습니다`);
  }

  if (violations.length > 0) return { violations };
  return { plan: { title: (title as string).trim(), project: project as string, cards: valid }, violations };
}

function checkCard(raw: unknown, add: (rule: string, message: string, cardId?: string) => void): Card | undefined {
  if (!isRecord(raw)) {
    add("plan-shape", "카드는 JSON 객체여야 합니다");
    return undefined;
  }
  const id = typeof raw.id === "string" ? raw.id : "?";
  let ok = true;
  const fail = (rule: string, message: string) => {
    ok = false;
    add(rule, message, id);
  };

  for (const key of Object.keys(raw)) {
    if (!(CARD_FIELDS as readonly string[]).includes(key)) {
      fail("unsupported-field", `카드에 허용되지 않는 필드: ${key} — 실행 파일·인자·환경변수·계정은 카드로 지정할 수 없습니다`);
    }
  }
  if (!CARD_ID.test(id)) fail("plan-shape", "카드 id는 영문 소문자·숫자·하이픈 32자 이내입니다");
  if (!isProvider(raw.provider)) fail("unknown-provider", `provider는 ${PROVIDERS.join(", ")} 중 하나여야 합니다`);
  if (raw.role !== undefined && (typeof raw.role !== "string" || raw.role.length > 40)) fail("plan-shape", "role은 40자 이내의 글자여야 합니다(선택 항목)");
  if (typeof raw.task !== "string" || !raw.task.trim()) fail("plan-shape", "task가 필요합니다");
  else if (raw.task.length > LIMITS.maxTaskChars) fail("plan-size", `task는 ${LIMITS.maxTaskChars}자 이내입니다`);
  if (raw.model !== undefined && (typeof raw.model !== "string" || !MODEL.test(raw.model))) fail("plan-shape", "model 이름 형식이 올바르지 않습니다");
  if (raw.effort !== undefined && !(EFFORTS as readonly unknown[]).includes(raw.effort)) fail("plan-shape", `effort는 ${EFFORTS.join(", ")} 중 하나입니다`);

  if (typeof raw.task === "string") {
    for (const hit of findHits(raw.task, CODE_RULES)) fail(`task-${hit.rule}`, hit.reason);
  }
  if (!ok) return undefined;
  return {
    id,
    provider: raw.provider as ProviderId,
    role: typeof raw.role === "string" && raw.role.trim() ? raw.role.trim() : undefined,
    task: raw.task as string,
    model: raw.model as string | undefined,
    effort: raw.effort as string | undefined,
  };
}

/** A team message passes the same credential, backend and bypass rules as a task. */
export function checkMessage(text: string, parties: { from: string; to: string; known: string[] }): Violation[] {
  const violations: Violation[] = [];
  if (!text.trim()) violations.push({ rule: "msg-shape", message: "빈 메시지는 보낼 수 없습니다" });
  if (text.length > LIMITS.maxMessageChars) violations.push({ rule: "msg-size", message: `메시지는 ${LIMITS.maxMessageChars}자 이내입니다` });
  for (const party of [parties.from, parties.to]) {
    if (!["lead", "all", ...parties.known].includes(party)) violations.push({ rule: "msg-party", message: `이 실행에 없는 상대입니다: ${party}` });
  }
  for (const hit of findHits(text, CODE_RULES)) violations.push({ rule: `msg-${hit.rule}`, message: hit.reason });
  return violations;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isAbsolute(path: string): boolean {
  return /^[A-Za-z]:[\\/]/.test(path) || path.startsWith("/");
}
