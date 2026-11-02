import { useState } from "react";

import type { ProviderId } from "../../../../src/core/plan.ts";
import { useCharacterSheet, type Look } from "./characters.ts";

/** Who speaks: an agent's provider, or the lead. Pixel portraits live in public/office/. */
export type Speaker = ProviderId | "lead";

const RING: Record<Speaker, string> = {
  claude: "ring-orange-400/60",
  codex: "ring-sky-400/60",
  agy: "ring-emerald-400/60",
  lead: "ring-neutral-400/60",
};

const INITIAL: Record<Speaker, string> = { claude: "Cl", codex: "Cx", agy: "Ag", lead: "총" };

/** Head and shoulders in a composed sheet's first idle frame: a square of CROP px from (CROP_X, CROP_Y). */
const SHEET_WIDTH = 1024;
const CROP = 128;
const CROP_X = 64;
const CROP_Y = 34;

export function spriteUrl(speaker: Speaker): string {
  return `/office/agent-${speaker}.png`;
}

/**
 * The speaker's pixel head in a round frame, ringed in the provider's colour. With a
 * look it's that card's composed character; until that is drawn (or if it can't be)
 * the provider's portrait, and the initials if the art is missing.
 */
export function Avatar({ speaker, size = "md", look }: { speaker: Speaker; size?: "sm" | "md"; look?: Look }) {
  const [missing, setMissing] = useState(false);
  const sheet = useCharacterSheet(look);
  const box = size === "sm" ? "size-6" : "size-8";
  return (
    <span className={`relative inline-flex shrink-0 items-center justify-center overflow-hidden rounded-full bg-muted ring-1 ${RING[speaker]} ${box}`}>
      {sheet ? (
        <img
          src={sheet}
          alt=""
          className="absolute max-w-none [image-rendering:pixelated]"
          style={{ width: `${(SHEET_WIDTH / CROP) * 100}%`, left: `${(-CROP_X / CROP) * 100}%`, top: `${(-CROP_Y / CROP) * 100}%` }}
        />
      ) : missing ? (
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
