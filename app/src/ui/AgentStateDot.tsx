/*
 * Adapted from Orca's AgentStateDot and AgentWorkingSpinner
 * (https://github.com/stablyai/orca, src/renderer/src/components/).
 * MIT License, Copyright (c) 2026 Lovecast Inc. See THIRD_PARTY_NOTICES.md.
 */
import { CircleCheck } from "lucide-react";

import type { CardState } from "../../../src/core/snapshot.ts";
import { STATE_WORD } from "../lib/format.ts";

/** The compact state glyph Orca uses on agent rows and tabs, keyed to O's card states. */
export function AgentStateDot({ state, size = "sm" }: { state: CardState; size?: "sm" | "md" }) {
  const box = size === "md" ? "h-3 w-3" : "h-2.5 w-2.5";
  const inner = size === "md" ? "size-2" : "size-1.5";
  const icon = size === "md" ? "size-3" : "size-2.5";
  const label = STATE_WORD[state];

  if (state === "running") {
    return (
      <span role="img" aria-label={label} title={label} className={`inline-flex shrink-0 items-center justify-center ${box}`}>
        <span className={`agent-working-spinner block rounded-full border-2 border-agent-working border-t-transparent motion-reduce:border-t-agent-working ${inner}`} />
      </span>
    );
  }
  if (state === "done") {
    return (
      <span role="img" aria-label={label} title={label} className={`inline-flex shrink-0 items-center justify-center ${box}`}>
        <CircleCheck className={`text-emerald-500 ${icon}`} aria-hidden="true" />
      </span>
    );
  }
  const dot = state === "failed" ? "bg-red-500" : state === "stopped" ? "bg-muted-foreground" : "bg-neutral-500/40";
  return (
    <span role="img" aria-label={label} title={label} className={`inline-flex shrink-0 items-center justify-center ${box}`}>
      <span className={`block rounded-full ${inner} ${dot}`} />
    </span>
  );
}
