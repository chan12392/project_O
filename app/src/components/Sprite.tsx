import { useEffect, useState } from "react";

import { spriteUrl, type Speaker } from "./Avatar.tsx";

/**
 * Actions drawn from two 4 x 4 sheets per character (public/office/sheets/sheets.json):
 * sheet a holds idle, walk, type and wave; sheet b holds coffee, think, cheer and stuck.
 */
export type Action = "idle" | "walk" | "type" | "wave" | "coffee" | "think" | "cheer" | "stuck";

const FRAME: Record<Action, { sheet: "a" | "b"; row: number; ms: number }> = {
  idle: { sheet: "a", row: 0, ms: 1400 },
  walk: { sheet: "a", row: 1, ms: 520 },
  type: { sheet: "a", row: 2, ms: 480 },
  wave: { sheet: "a", row: 3, ms: 760 },
  coffee: { sheet: "b", row: 0, ms: 1600 },
  think: { sheet: "b", row: 1, ms: 1500 },
  cheer: { sheet: "b", row: 2, ms: 560 },
  stuck: { sheet: "b", row: 3, ms: 1100 },
};

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
 * A character stepping through one row of its sheet. A missing sheet b falls
 * back to sheet a's idle row; no sheets at all falls back to the standing portrait.
 */
export function Sprite({ speaker, action, flip, paused }: { speaker: Speaker; action: Action; flip?: boolean; paused?: boolean }) {
  const ready = useSheets(speaker);
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
