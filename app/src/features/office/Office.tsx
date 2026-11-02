import { CircleCheckBig, Pause, Play } from "lucide-react";
import { useEffect, useState } from "react";

import { isSessionRun } from "../../../../src/core/attention.ts";
import { chipSummary } from "../../../../src/core/projects.ts";
import type { CardView, RunView } from "../../../../src/core/snapshot.ts";
import { CardSummary } from "./CardSummary.tsx";
import { PROVIDER_SHORT, STATE_WORD, clip } from "../../lib/format.ts";
import { lookOf } from "./characters.ts";
import { useReducedMotion } from "../../data/settings.ts";
import { AgentStateDot } from "../../ui/AgentStateDot.tsx";
import { Person, type Mood, type Point } from "./Person.tsx";

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
/*
 * Where people wander, kept as constants: a fresh array each render would
 * restart a person's wander timer on every poll, so it would never fire.
 */
const STAY: Point[] = [];
const AFTER_WORK: Point[] = [...LOUNGE, ...MEETING];
const LEAD_ROUND: Point[] = [BOARD, AISLE[0]];

/** The overlay lists each card up to this many; more fold into a summary line. */
const LISTED_CARDS = 6;

/** Where a card's agent should be for its state. */
function homeFor(card: CardView, index: number): { spot: Point; roam: Point[] } {
  const desk = DESKS[index % DESKS.length];
  if (card.state === "running" || card.state === "failed") return { spot: desk, roam: STAY };
  if (card.state === "done") return { spot: LOUNGE[index % LOUNGE.length], roam: AFTER_WORK };
  return { spot: AISLE[index % AISLE.length], roam: AISLE };
}

/** The office: agents at their desks while they work, wandering otherwise, saying what they do. */
export function Office({ run, selected, leadResting, onSelect }: { run: RunView; selected?: string; leadResting?: boolean; onSelect: (id: string) => void }) {
  const finished = run.cards.filter((c) => c.endedAt).length;
  const summary = run.cards.length > LISTED_CARDS ? chipSummary(run, new Date()) : undefined;
  // A Claude Code session: its main thread stands in the lead's place, each subagent is a person.
  const session = isSessionRun(run);
  const lead = session ? "메인 대화" : "총괄";
  // Reduced motion (Settings → 모양, else the system) starts the office held; the button still overrides it.
  const reduced = useReducedMotion();
  const [held, setHeld] = useState(reduced);
  useEffect(() => setHeld(reduced), [reduced]);
  return (
    <div>
      <div className="flex items-center justify-between gap-3 px-5 pt-4">
        <p className="text-xs text-muted-foreground">
          {session
            ? "Claude Code 세션: 메인 대화가 총괄 자리에 서고, 서브에이전트마다 한 사람이 일합니다. 끝나면 휴게 공간으로 갑니다."
            : "카드마다 한 사람. 일하면 자기 책상에, 끝나면 휴게 공간으로 갑니다. 총괄은 할 일이 없으면 쉽니다."}
        </p>
        <button
          type="button"
          onClick={() => setHeld((h) => !h)}
          className="flex h-7 shrink-0 items-center gap-1.5 rounded-md border px-2 text-xs hover:bg-accent focus-ring"
        >
          {held ? <Play className="size-3.5" aria-hidden="true" /> : <Pause className="size-3.5" aria-hidden="true" />}
          {held ? "움직임 켜기" : "움직임 멈추기"}
        </button>
      </div>
      <div className="p-5">
        <div
          role="group"
          aria-label={`사무실: ${session ? "메인 대화와 서브에이전트마다" : "카드마다"} 한 사람이 상태에 따라 움직이는 다이어그램`}
          className="relative mx-auto aspect-[3/2] w-full max-w-[960px] overflow-hidden rounded-xl border bg-amber-950/40"
        >
          <img src="/office/office.png" alt="" className="absolute inset-0 size-full object-cover [image-rendering:pixelated]" onError={(e) => (e.currentTarget.style.display = "none")} />
          <Person
            speaker="lead"
            label={leadResting ? `${lead} · 쉬는 중` : lead}
            spot={leadResting ? REST : BOARD}
            roam={leadResting ? STAY : LEAD_ROUND}
            held={held}
            mood={leadResting ? "rest" : "lead"}
            line={leadResting ? undefined : latestLine(run, "lead")}
          />
          <div className="absolute top-2 right-2 z-[150] max-w-[45%] rounded-lg bg-black/55 px-2.5 py-1.5 text-white shadow-sm backdrop-blur-[2px]" role="status" aria-label={`완료 ${finished}/${run.cards.length}`}>
            <p className="flex items-center gap-1.5 text-xs font-semibold tabular-nums">
              <CircleCheckBig className="size-3.5 text-emerald-400" aria-hidden="true" />
              완료 {finished}/{run.cards.length}
            </p>
            {/* A session with dozens of subagents: one summary line, then only the cards that need a look. */}
            {summary && <CardSummary summary={summary} onDark className="mt-1" />}
            <ul className="mt-1 grid gap-0.5 text-3xs text-white/80">
              {(summary ? summary.attention.map((a) => a.card) : run.cards).map((card) => (
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
                look={lookOf(run, card.id)}
                label={personLabel(card, session)}
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

/**
 * A subagent goes by its type and what it was asked to do ("Explore · 코드 찾기"); a card by its label.
 * The default type says nothing, and its long name would push the label out of the room.
 */
function personLabel(card: CardView, session: boolean): string {
  if (!session || !card.role || card.role === card.label || card.role === "general-purpose") return card.label;
  return `${card.role} · ${clip(card.label, 20)}`;
}

/** What a person last said: their newest team message, else what their card last did. */
function latestLine(run: RunView, party: string, card?: CardView): { key: string; text: string } | undefined {
  const message = [...run.messages].reverse().find((m) => m.from === party);
  const activity = card && card.state === "running" && card.activity ? { key: `a:${card.activity}`, text: card.activity } : undefined;
  if (card?.state === "failed") return { key: "failed", text: "!" };
  if (message && (!card?.log.length || message.at >= (card.log[card.log.length - 1]?.at ?? ""))) return { key: `m:${message.at}`, text: message.text };
  return activity ?? (message ? { key: `m:${message.at}`, text: message.text } : undefined);
}

function moodOf(card: CardView): Mood {
  if (card.state === "running") return "work";
  if (card.state === "failed") return "fail";
  if (card.state === "done" || card.state === "stopped") return "done";
  return "wait";
}
