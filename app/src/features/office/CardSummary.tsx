import type { ChipSummary, SummaryState } from "../../../../src/core/projects.ts";
import { PROVIDER_SHORT, STATE_WORD } from "../../lib/format.ts";
import { AgentStateDot } from "../../ui/AgentStateDot.tsx";
import { Avatar } from "./Avatar.tsx";

/** One line for a run's cards: each agent once with its count, then how many are in each state. */
export function CardSummary({ summary, className = "", onDark = false }: { summary: ChipSummary; className?: string; onDark?: boolean }) {
  return (
    <span className={`flex min-w-0 flex-wrap items-center gap-x-2 gap-y-0.5 text-3xs ${onDark ? "text-white/80" : "text-muted-foreground"} ${className}`}>
      {summary.providers.map((p) => (
        <span key={p.provider} className="flex items-center gap-1 whitespace-nowrap">
          <Avatar speaker={p.provider} size="xs" />
          <span className="font-semibold">
            {PROVIDER_SHORT[p.provider]}
            {p.count > 1 && <span className="font-normal tabular-nums"> ×{p.count}</span>}
          </span>
        </span>
      ))}
      {summary.states.length > 0 && (
        <span className="flex items-center gap-1.5 whitespace-nowrap">
          {summary.states.map((s) => (
            <span key={s.state} className="flex items-center gap-0.5 tabular-nums" title={`${STATE_WORD[s.state]} ${s.count}`}>
              <AgentStateDot state={s.state} />
              {s.count}
            </span>
          ))}
        </span>
      )}
    </span>
  );
}

/** "Codex 4장 · Claude 1장, 완료 3 · 진행 1": the same line for screen readers. */
export function summaryWords(summary: ChipSummary): string {
  const agents = summary.providers.map((p) => `${PROVIDER_SHORT[p.provider]} ${p.count}장`).join(" · ");
  const states = summary.states.map((s) => `${STATE_WORD[s.state as SummaryState]} ${s.count}`).join(" · ");
  return [agents, states].filter(Boolean).join(", ");
}
