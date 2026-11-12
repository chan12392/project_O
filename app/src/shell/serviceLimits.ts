import type { ProviderId } from "../../../src/core/plan.ts";
import type { CardView } from "../../../src/core/snapshot.ts";
import { PROVIDER_NAME } from "../lib/format.ts";
import type { ProviderBudget } from "../platform/backend/types.ts";

/**
 * The status bar's limit line, beside the usage chips (features/usage/statusUsage.ts), which carry
 * each agent's subscription windows. It names what the chips cannot: a subscription limit a card's
 * CLI hit (`limit` entries in its log) and today's budget (ADR-0004) used up, which the machine
 * reports and which refuses new cards until the next day even when no card has hit anything.
 * With nothing to report the text is empty and the bar leaves the line out.
 */
export function serviceLimits(cards: readonly CardView[], budget: readonly Pick<ProviderBudget, "provider" | "state" | "message">[] | undefined): { text: string; title?: string } {
  const names = (providers: Iterable<ProviderId>) => [...new Set(providers)].map((p) => PROVIDER_NAME[p]).join(", ");
  const limited = cards.filter((c) => c.log.some((l) => l.kind === "limit")).map((c) => c.provider);
  // The daily budget switched off (ADR-0013): every provider reads "off", nothing is refused or waited on.
  const budgetOff = !!budget && budget.length > 0 && budget.every((b) => b.state === "off");
  const over = (budget ?? []).filter((b) => b.state === "over");
  const parts = [limited.length > 0 && `구독 한도에 걸린 카드: ${names(limited)}`, over.length > 0 && `오늘 예산 소진: ${names(over.map((b) => b.provider))}`].filter(Boolean);
  if (!budgetOff && (!budget || budget.some((b) => b.state === "unknown"))) parts.push("일부 오늘 예산 확인 전");
  if (parts.length === 0) {
    return budgetOff ? { text: "하루 예산 꺼짐", title: "하루 사용 예산이 꺼져 있어 예산으로 막거나 effort를 낮추지 않습니다 (설정 → 에이전트)" } : { text: "" };
  }
  const title = [limited.length > 0 && "카드 실행 중 CLI가 구독 사용 한도에 걸림", ...over.map((b) => b.message)].filter(Boolean).join("\n");
  return { text: parts.join(" · "), title };
}
