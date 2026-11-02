import { useEffect, useState } from "react";

import { useCharacterSheet, type Look } from "./characters.ts";
import { spriteUrl, type Speaker } from "./Avatar.tsx";

/**
 * Actions drawn from two 4 x 4 sheets per character (public/office/sheets/sheets.json):
 * sheet a holds idle, walk, type and wave; sheet b holds coffee, think, cheer and stuck.
 * Walking toward the viewer (walkDown) or away (walkUp) has its own rows only in
 * composed sheets; drawn sheets walk sideways instead.
 */
export type Action = "idle" | "walk" | "walkDown" | "walkUp" | "type" | "wave" | "coffee" | "think" | "cheer" | "stuck" | "talk";

const FRAME: Record<Action, { sheet: "a" | "b"; row: number; ms: number }> = {
  idle: { sheet: "a", row: 0, ms: 1400 },
  walk: { sheet: "a", row: 1, ms: 520 },
  walkDown: { sheet: "a", row: 1, ms: 520 },
  walkUp: { sheet: "a", row: 1, ms: 520 },
  type: { sheet: "a", row: 2, ms: 480 },
  wave: { sheet: "a", row: 3, ms: 760 },
  coffee: { sheet: "b", row: 0, ms: 1600 },
  think: { sheet: "b", row: 1, ms: 1500 },
  cheer: { sheet: "b", row: 2, ms: 560 },
  stuck: { sheet: "b", row: 3, ms: 1100 },
  talk: { sheet: "a", row: 0, ms: 1400 },
};

/** Rows of a composed character's 4 x 12 sheet (move, act1, act2; see characters.ts). */
const COMPOSED_ROW: Record<Action, number> = { idle: 0, walk: 1, walkDown: 2, walkUp: 3, type: 5, wave: 6, talk: 7, coffee: 8, think: 9, cheer: 10, stuck: 11 };

function sheetUrl(speaker: Speaker, sheet: "a" | "b"): string {
  return `/office/sheets/sheet${sheet === "b" ? "-b" : ""}-${speaker}.png`;
}

const loaded = new Map<string, Promise<boolean>>();

/** Whether a sheet exists, asked once per file. */
function sheetReady(url: string): Promise<boolean> {
  if (!loaded.has(url)) {
    loaded.set(
      url,
      new Promise((resolve) => {
        const img = new Image();
        img.onload = () => resolve(true);
        img.onerror = () => resolve(false);
        img.src = url;
      }),
    );
  }
  return loaded.get(url)!;
}

function useSheets(speaker: Speaker) {
  const [ready, setReady] = useState({ a: false, b: false });
  useEffect(() => {
    let live = true;
    Promise.all([sheetReady(sheetUrl(speaker, "a")), sheetReady(sheetUrl(speaker, "b"))]).then(([a, b]) => live && setReady({ a, b }));
    return () => {
      live = false;
    };
  }, [speaker]);
  return ready;
}

/**
 * A character stepping through one row of its sheet. With a look, the sheet is
 * composed from parts; until it is drawn (or if it can't be) the speaker's own
 * sheets show. A missing sheet b falls back to sheet a's idle row; no sheets at
 * all falls back to the standing portrait.
 */
export function Sprite({ speaker, action, flip, paused, look }: { speaker: Speaker; action: Action; flip?: boolean; paused?: boolean; look?: Look }) {
  const ready = useSheets(speaker);
  const composed = useCharacterSheet(look);
  if (composed) {
    return (
      <span
        aria-hidden="true"
        className={`sprite-sheet block aspect-square w-full ${flip ? "-scale-x-100" : ""}`}
        style={{
          backgroundImage: `url(${composed})`,
          backgroundSize: "400% 1200%",
          backgroundPositionY: `${(COMPOSED_ROW[action] * 100) / 11}%`,
          animationDuration: `${FRAME[action].ms}ms`,
          animationPlayState: paused ? "paused" : "running",
        }}
      />
    );
  }
  const wanted = FRAME[action];
  const frame = ready[wanted.sheet] ? wanted : FRAME.idle;

  if (!ready.a) {
    return <img src={spriteUrl(speaker)} alt="" className={`w-full [image-rendering:pixelated] ${flip ? "-scale-x-100" : ""}`} />;
  }
  return (
    <span
      aria-hidden="true"
      className={`sprite-sheet block aspect-square w-full ${flip ? "-scale-x-100" : ""}`}
      style={{
        backgroundImage: `url(${sheetUrl(speaker, frame.sheet)})`,
        backgroundPositionY: `${(frame.row * 100) / 3}%`,
        animationDuration: `${frame.ms}ms`,
        animationPlayState: paused ? "paused" : "running",
      }}
    />
  );
}
