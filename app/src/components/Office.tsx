import { CircleCheckBig, Pause, Play } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import type { CardView, RunView } from "../../../src/core/snapshot.ts";
import { PROVIDER_SHORT, STATE_WORD, clip } from "../format.ts";
import { AgentStateDot } from "./AgentStateDot.tsx";
import type { Speaker } from "./Avatar.tsx";
import { lookFor, type Look } from "../characters.ts";
import { Sprite, type Action } from "./Sprite.tsx";

const REDUCED = typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches;

type Point = { x: number; y: number };

/**
 * Where people's feet go in public/office/office.png (1536 x 1024), as
 * percentages of the room. Desks are the four chairs; the rest are places to wander.
 */
const DESKS: Point[] = [
  { x: 14, y: 50 },
  { x: 30, y: 50 },
  { x: 14, y: 80 },
  { x: 30, y: 80 },
];
const LOUNGE: Point[] = [
  { x: 79, y: 37 },
  { x: 71, y: 50 },
  { x: 81, y: 55 },
];
const MEETING: Point[] = [
  { x: 64, y: 78 },
  { x: 90, y: 74 },
];
const AISLE: Point[] = [
  { x: 47, y: 42 },
  { x: 55, y: 62 },
  { x: 46, y: 86 },
  { x: 61, y: 48 },
];
const BOARD: Point = { x: 45, y: 33 };
/** Where a resting lead has its coffee, apart from the lounge the finished agents use. */
const REST: Point = { x: 88, y: 40 };

/** Where a card's agent should be for its state. */
function homeFor(card: CardView, index: number): { spot: Point; roam: Point[] } {
  const desk = DESKS[index % DESKS.length];
  if (card.state === "running" || card.state === "failed") return { spot: desk, roam: [] };
  if (card.state === "done") return { spot: LOUNGE[index % LOUNGE.length], roam: [...LOUNGE, ...MEETING] };
  return { spot: AISLE[index % AISLE.length], roam: AISLE };
}

/** The office: agents at their desks while they work, wandering otherwise, saying what they do. */
export function Office({ run, selected, leadResting, onSelect }: { run: RunView; selected?: string; leadResting?: boolean; onSelect: (id: string) => void }) {
  const finished = run.cards.filter((c) => c.endedAt).length;
  const [held, setHeld] = useState(REDUCED);
  return (
    <div>
      <div className="flex items-center justify-between gap-3 px-5 pt-4">
        <p className="text-xs text-muted-foreground">카드마다 한 사람. 일하면 자기 책상에, 끝나면 휴게 공간으로 갑니다. 총괄은 할 일이 없으면 쉽니다.</p>
        <button
          type="button"
          onClick={() => setHeld((h) => !h)}
          className="flex h-7 shrink-0 items-center gap-1.5 rounded-md border px-2 text-xs hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
        >
          {held ? <Play className="size-3.5" aria-hidden="true" /> : <Pause className="size-3.5" aria-hidden="true" />}
          {held ? "움직임 켜기" : "움직임 멈추기"}
        </button>
      </div>
      <div className="p-5">
        <div
          role="group"
          aria-label="사무실: 카드마다 한 사람이 상태에 따라 움직이는 다이어그램"
          className="relative mx-auto aspect-[3/2] w-full max-w-[960px] overflow-hidden rounded-xl border bg-amber-950/40"
        >
          <img src="/office/office.png" alt="" className="absolute inset-0 size-full object-cover [image-rendering:pixelated]" onError={(e) => (e.currentTarget.style.display = "none")} />
          <Person
            speaker="lead"
            label={leadResting ? "총괄 · 쉬는 중" : "총괄"}
            spot={leadResting ? REST : BOARD}
            roam={leadResting ? [] : [BOARD, AISLE[0]]}
            held={held}
            mood={leadResting ? "rest" : "lead"}
            line={leadResting ? undefined : latestLine(run, "lead")}
          />
          <div className="absolute top-2 right-2 z-[150] max-w-[45%] rounded-lg bg-black/55 px-2.5 py-1.5 text-white shadow-sm backdrop-blur-[2px]" role="status" aria-label={`완료 ${finished}/${run.cards.length}`}>
            <p className="flex items-center gap-1.5 text-xs font-semibold tabular-nums">
              <CircleCheckBig className="size-3.5 text-emerald-400" aria-hidden="true" />
              완료 {finished}/{run.cards.length}
            </p>
            <ul className="mt-1 grid gap-0.5 text-3xs text-white/80">
              {run.cards.map((card) => (
                <li key={card.id} className="flex min-w-0 items-center gap-1.5">
                  <AgentStateDot state={card.state} />
                  <span className="truncate text-white">{card.label}</span>
                  <span className="ml-auto shrink-0 pl-1">
                    {PROVIDER_SHORT[card.provider]} {STATE_WORD[card.state]}
                  </span>
                </li>
              ))}
            </ul>
          </div>
          {run.cards.map((card, i) => {
            const home = homeFor(card, i);
            return (
              <Person
                key={card.id}
                speaker={card.provider}
                look={lookFor(card.id)}
                label={card.label}
                spot={home.spot}
                roam={home.roam}
                held={held}
                mood={moodOf(card)}
                line={latestLine(run, card.id, card)}
                selected={card.id === selected}
                onClick={() => onSelect(card.id)}
                nudge={card.state === "running" || card.state === "failed" ? 0 : ((i % 3) - 1) * 4}
              />
            );
          })}
        </div>
      </div>
    </div>
  );
}

/** What a person last said: their newest team message, else what their card last did. */
function latestLine(run: RunView, party: string, card?: CardView): { key: string; text: string } | undefined {
  const message = [...run.messages].reverse().find((m) => m.from === party);
  const activity = card && card.state === "running" && card.activity ? { key: `a:${card.activity}`, text: card.activity } : undefined;
  if (card?.state === "failed") return { key: "failed", text: "!" };
  if (message && (!card?.log.length || message.at >= (card.log[card.log.length - 1]?.at ?? ""))) return { key: `m:${message.at}`, text: message.text };
  return activity ?? (message ? { key: `m:${message.at}`, text: message.text } : undefined);
}

/** What a person is up to, from their card's state: it picks the sheet row they play. */
type Mood = "work" | "fail" | "wait" | "done" | "lead" | "rest";

function moodOf(card: CardView): Mood {
  if (card.state === "running") return "work";
  if (card.state === "failed") return "fail";
  if (card.state === "done" || card.state === "stopped") return "done";
  return "wait";
}

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

function Person({ speaker, look, label, spot, roam, held, mood, line, selected, onClick, nudge = 0 }: PersonProps) {
  const [pos, setPos] = useState(spot);
  const [facingLeft, setFacingLeft] = useState(false);
  const [walking, setWalking] = useState(false);
  const [speaking, setSpeaking] = useState(false);
  const [waving, setWaving] = useState(false);
  const [cheering, setCheering] = useState(false);
  const [sipping, setSipping] = useState(false);
  const last = useRef(spot);
  const lastMood = useRef(mood);
  const alert = mood === "fail";

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
      setFacingLeft(next.x < last.current.x);
      setWalking(true);
      last.current = next;
      setPos(next);
    },
    [nudge],
  );

  useEffect(() => goTo(spot), [spot.x, spot.y, goTo]);

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
    ? "walk"
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
        className="block w-full rounded-md focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none disabled:cursor-default aria-pressed:ring-2 aria-pressed:ring-ring"
      >
        <Sprite speaker={speaker} look={look} action={action} flip={action === "walk" && facingLeft} paused={held} />
      </button>
      <span className="pointer-events-none absolute top-full left-1/2 mt-0.5 -translate-x-1/2 rounded bg-black/60 px-1.5 text-3xs whitespace-nowrap text-white">{label}</span>
    </div>
  );
}
