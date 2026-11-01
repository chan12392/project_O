import { useState } from "react";

import type { ProviderId } from "../../../src/core/plan.ts";

/** Who speaks: an agent's provider, or the lead. Pixel portraits live in public/office/. */
export type Speaker = ProviderId | "lead";

const RING: Record<Speaker, string> = {
  claude: "ring-orange-400/60",
  codex: "ring-sky-400/60",
  agy: "ring-emerald-400/60",
  lead: "ring-neutral-400/60",
};

const INITIAL: Record<Speaker, string> = { claude: "Cl", codex: "Cx", agy: "Ag", lead: "총" };

export function spriteUrl(speaker: Speaker): string {
  return `/office/agent-${speaker}.png`;
}

/** The speaker's pixel head in a round frame; their initials if the art is missing. */
export function Avatar({ speaker, size = "md" }: { speaker: Speaker; size?: "sm" | "md" }) {
  const [missing, setMissing] = useState(false);
  const box = size === "sm" ? "size-6" : "size-8";
  return (
    <span className={`relative inline-flex shrink-0 items-center justify-center overflow-hidden rounded-full bg-muted ring-1 ${RING[speaker]} ${box}`}>
      {missing ? (
        <span className="text-3xs font-semibold text-muted-foreground">{INITIAL[speaker]}</span>
      ) : (
        <img
          src={spriteUrl(speaker)}
          alt=""
          onError={() => setMissing(true)}
          className="absolute -top-[6%] left-1/2 w-[150%] max-w-none -translate-x-1/2 [image-rendering:pixelated]"
        />
      )}
    </span>
  );
}
