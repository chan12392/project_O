/**
 * Where every node of a graph sits (ADR-0041): the whole force layout computed in one go,
 * so the canvas only ever draws finished positions. `layoutWorker.ts` runs this off the
 * main thread — a slower start instead of a stuttering one. Pure: the same input gives the
 * same positions.
 */
// @ts-expect-error d3-force-3d ships no type declarations.
import { forceCenter, forceLink, forceManyBody, forceSimulation, forceX, forceY, forceZ } from "d3-force-3d";

export interface LayoutInput {
  /** How many nodes; positions come back in the same order. */
  count: number;
  /** x, y, z per node: where it already sits, or NaN for the layout to choose. */
  seed: Float32Array;
  /** Index pairs (source, target) into the nodes. */
  links: Uint32Array;
}

interface SimNode {
  x?: number;
  y?: number;
  z?: number;
}

/** The ticks a fresh layout runs (d3's default cooling), and a settled one that only gained nodes. */
const FRESH_TICKS = 300;
const SETTLED_TICKS = 140;
/** Past this many nodes each tick is slow enough to shorten the run. */
const LARGE = 3000;

/** x, y, z per node. Nodes with a seed start there, so a graph that only gained a few barely moves. */
export function layout({ count, seed, links }: LayoutInput): Float32Array {
  const out = new Float32Array(count * 3);
  if (count === 0) return out;
  let seeded = 0;
  const nodes: SimNode[] = Array.from({ length: count }, (_, i) => {
    const [x, y, z] = [seed[i * 3], seed[i * 3 + 1], seed[i * 3 + 2]];
    if (![x, y, z].every(Number.isFinite)) return {};
    seeded++;
    return { x, y, z };
  });
  const pairs: { source: number; target: number }[] = [];
  for (let i = 0; i + 1 < links.length; i += 2) {
    if (links[i] < count && links[i + 1] < count && links[i] !== links[i + 1]) pairs.push({ source: links[i], target: links[i + 1] });
  }
  // The forces 3d-force-graph ran before, plus a weak pull to the middle so loose nodes do not drift off.
  const simulation = forceSimulation(nodes, 3)
    .force("link", forceLink(pairs).distance(30))
    .force("charge", forceManyBody())
    .force("center", forceCenter())
    .force("x", forceX().strength(0.03))
    .force("y", forceY().strength(0.03))
    .force("z", forceZ().strength(0.03))
    .stop();
  const settled = seeded > count / 2;
  if (settled) simulation.alpha(0.4);
  const ticks = Math.round((settled ? SETTLED_TICKS : FRESH_TICKS) * (count > LARGE ? 0.6 : 1));
  simulation.tick(ticks);
  nodes.forEach((n, i) => {
    out[i * 3] = Number.isFinite(n.x) ? n.x! : 0;
    out[i * 3 + 1] = Number.isFinite(n.y) ? n.y! : 0;
    out[i * 3 + 2] = Number.isFinite(n.z) ? n.z! : 0;
  });
  return out;
}
