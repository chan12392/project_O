import { useEffect, useMemo, useRef, useState } from "react";

import type { Look } from "./characters.ts";
import { clip } from "../../lib/format.ts";
import { useReducedMotion } from "../../data/settings.ts";
import type { Speaker } from "./Avatar.tsx";
import { Sprite, type Action } from "./Sprite.tsx";

/** A place in the office, in percent of the room. */
export type Point = { x: number; y: number };

/** What a person is up to, from their card's state: it picks the sheet row they play. */
export type Mood = "work" | "fail" | "wait" | "done" | "lead" | "rest";

interface PersonProps {
  speaker: Speaker;
  /** A composed look; the lead keeps its own drawn sheets. */
  look?: Look;
  label: string;
  spot: Point;
  roam: Point[];
  held: boolean;
  mood: Mood;
  line?: { key: string; text: string };
  selected?: boolean;
  onClick?: () => void;
  /** Sideways offset in room percent, so people sharing a spot don't overlap. */
  nudge?: number;
}

const SHOW_LINE_MS = 6000;
const WAVE_MS = 2400;
const CHEER_MS = 3200;

export function Person({ speaker, look, label, spot, roam, held, mood, line, selected, onClick, nudge = 0 }: PersonProps) {
  const [pos, setPos] = useState(spot);
  const [facingLeft, setFacingLeft] = useState(false);
  const [stride, setStride] = useState<"walk" | "walkDown" | "walkUp">("walk");
  const [walking, setWalking] = useState(false);
  const [speaking, setSpeaking] = useState(false);
  const [waving, setWaving] = useState(false);
  const [cheering, setCheering] = useState(false);
  const [sipping, setSipping] = useState(false);
  const last = useRef(spot);
  const lastMood = useRef(mood);
  const alert = mood === "fail";
  const reduced = useReducedMotion();

  // Finishing earns a short cheer; after that a done agent sips coffee now and then.
  useEffect(() => {
    if (lastMood.current !== "done" && mood === "done") {
      setCheering(true);
      const id = window.setTimeout(() => setCheering(false), CHEER_MS);
      lastMood.current = mood;
      return () => window.clearTimeout(id);
    }
    lastMood.current = mood;
  }, [mood]);
  useEffect(() => {
    if (mood !== "done" || held) return;
    const id = window.setInterval(() => setSipping((s) => !s), 5200);
    return () => window.clearInterval(id);
  }, [mood, held]);

  const goTo = useMemo(
    () => (target: Point) => {
      // People heading for the same spot stand a step apart instead of on top of each other.
      const next = { x: target.x + nudge, y: target.y };
      if (next.x === last.current.x && next.y === last.current.y) return;
      // Room percent across is 1.5 times percent down (the room is 3:2); a mostly vertical
      // move walks toward the viewer or away instead of sideways.
      const across = (next.x - last.current.x) * 1.5;
      const down = next.y - last.current.y;
      setFacingLeft(across < 0);
      setStride(Math.abs(down) > Math.abs(across) ? (down > 0 ? "walkDown" : "walkUp") : "walk");
      // Held or with reduced motion there is no glide (and no transitionend), so no walk either.
      setWalking(!held && !reduced);
      last.current = next;
      setPos(next);
    },
    [nudge, held, reduced],
  );

  useEffect(() => goTo(spot), [spot.x, spot.y, goTo]);
  // Holding drops the glide mid-walk; the person stops where they are headed.
  useEffect(() => {
    if (held) setWalking(false);
  }, [held]);

  useEffect(() => {
    if (held || roam.length < 2) return;
    const id = window.setInterval(() => goTo(roam[Math.floor(Math.random() * roam.length)]), 4200 + Math.random() * 2400);
    return () => window.clearInterval(id);
  }, [held, roam, goTo]);

  useEffect(() => {
    if (!line) return;
    setSpeaking(true);
    if (line.text === "!") return;
    // A new team message gets a wave; activity lines of a working agent don't interrupt the typing.
    if (line.key.startsWith("m:")) setWaving(true);
    const wave = window.setTimeout(() => setWaving(false), WAVE_MS);
    const id = window.setTimeout(() => setSpeaking(false), SHOW_LINE_MS);
    return () => {
      window.clearTimeout(id);
      window.clearTimeout(wave);
    };
  }, [line?.key]);

  const moving = walking && !held;
  const action: Action = moving
    ? stride
    : mood === "work"
      ? "type"
      : alert
        ? "stuck"
        : cheering
          ? "cheer"
          : waving
            ? "wave"
            : speaking && mood !== "wait"
              ? "talk"
              : mood === "wait"
                ? "think"
                : mood === "rest" || (mood === "done" && sipping)
                  ? "coffee"
                  : "idle";
  return (
    <div
      className="person absolute w-[10%] -translate-x-1/2 -translate-y-[94%]"
      style={{ left: `${pos.x}%`, top: `${pos.y}%`, transition: held ? "none" : "left 2.6s linear, top 2.6s linear", zIndex: Math.round(pos.y) }}
      onTransitionEnd={() => setWalking(false)}
      // A glide retargeted mid-walk is cancelled too, but a new one is already running then.
      onTransitionCancel={(e) => e.currentTarget.getAnimations().length === 0 && setWalking(false)}
    >
      {speaking && line && (
        <p
          role="status"
          className={`absolute w-max max-w-[13rem] rounded-xl border px-2.5 py-1.5 text-xs shadow-sm ${
            // Under the completion box (top right) the bubble hangs below the person instead.
            pos.x > 55 && pos.y < 50 ? "top-full mt-5" : "bottom-full mb-1"
          } ${
            // Keep the bubble inside the room: anchor it to the side away from the nearer wall.
            pos.x < 30 ? "left-0" : pos.x > 70 ? "right-0" : "left-1/2 -translate-x-1/2"
          } ${
            alert ? "border-red-500/60 bg-red-950 text-red-100" : "bg-popover text-popover-foreground"
          }`}
        >
          {alert ? "막혔어요! 기록을 봐 주세요" : clip(line.text, 70)}
        </p>
      )}
      <button
        type="button"
        onClick={onClick}
        disabled={!onClick}
        aria-label={`${label}${mood === "work" ? ", 작업 중" : alert ? ", 막힘" : ""}`}
        aria-pressed={onClick ? selected : undefined}
        className="block w-full rounded-md focus-ring disabled:cursor-default aria-pressed:ring-2 aria-pressed:ring-ring"
      >
        {/* Drawn sheets walk sideways in every direction, so they turn whenever they walk. */}
        <Sprite speaker={speaker} look={look} action={action} flip={facingLeft && (action === "walk" || (moving && !look))} paused={held} />
      </button>
      <span className="pointer-events-none absolute top-full left-1/2 mt-0.5 -translate-x-1/2 rounded bg-black/60 px-1.5 text-3xs whitespace-nowrap text-white">{label}</span>
    </div>
  );
}
