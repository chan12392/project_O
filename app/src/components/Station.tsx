import type { CardView } from "../../../src/core/snapshot.ts";
import { PROVIDER_NAME, STATE_WORD, clip, modelLine } from "../format.ts";
import { HEIGHT, POSES, TYPING, WIDTH, type Rect } from "../sprite.ts";

function Pixels({ rects }: { rects: Rect[] }) {
  return (
    <>
      {rects.map((r, i) => (
        <rect key={i} className={`px-${r.key}`} x={r.x} y={r.y} width={r.w} height={1} />
      ))}
    </>
  );
}

/** One desk in the office: the card's agent, drawn in the card's real state. */
export function Station({ card, selected, onSelect }: { card: CardView; selected: boolean; onSelect: (id: string) => void }) {
  const running = card.state === "running";
  return (
    <button
      type="button"
      className={`station station--${card.provider} is-${card.state}`}
      aria-pressed={selected}
      onClick={() => onSelect(card.id)}
    >
      <svg className="sprite" viewBox={`0 0 ${WIDTH} ${HEIGHT}`} shapeRendering="crispEdges" aria-hidden="true">
        {running ? (
          <>
            <g className="frame frame--a">
              <Pixels rects={TYPING[0]} />
            </g>
            <g className="frame frame--b">
              <Pixels rects={TYPING[1]} />
            </g>
          </>
        ) : (
          <Pixels rects={card.state === "done" ? POSES.done : POSES.rest} />
        )}
      </svg>
      <span className="station__sheet">
        <span className="station__role">{card.role}</span>
        <span className="station__who">
          <span>{PROVIDER_NAME[card.provider]}</span>
          <span className="station__model">{modelLine(card.model, card.effort)}</span>
        </span>
        <span className="station__state">{STATE_WORD[card.state]}</span>
        {card.activity && <span className="station__activity">{clip(card.activity, 64)}</span>}
      </span>
    </button>
  );
}
