/**
 * The arithmetic behind the canvas's motion (ADR-0041): when a node starts growing, how far
 * the camera stands back, which node the pointer is on. Pure, so it is testable without WebGL.
 */

/** Fast out, soft landing: 0 → 1. */
export function easeOut(t: number): number {
  const c = Math.min(1, Math.max(0, t));
  return 1 - (1 - c) ** 3;
}

/** The longest a reveal waits before its deepest level starts. */
export const REVEAL_SPREAD_MS = 1_100;
/** How long one node takes to grow out of its parent. */
export const GROW_MS = 700;

/** When a node of this depth starts growing: level by level, the whole spread bounded however deep the tree is. */
export function revealDelay(level: number, deepest: number): number {
  if (level <= 0 || deepest <= 0) return 0;
  return Math.round(Math.min(level, deepest) * Math.min(260, REVEAL_SPREAD_MS / deepest));
}

export interface Bounds {
  center: [number, number, number];
  radius: number;
}

/** The sphere around the first `count` positions (x, y, z each). */
export function boundsOf(positions: ArrayLike<number>, count: number): Bounds {
  if (count <= 0) return { center: [0, 0, 0], radius: 0 };
  let [cx, cy, cz] = [0, 0, 0];
  for (let i = 0; i < count; i++) {
    cx += positions[i * 3];
    cy += positions[i * 3 + 1];
    cz += positions[i * 3 + 2];
  }
  cx /= count;
  cy /= count;
  cz /= count;
  let far = 0;
  for (let i = 0; i < count; i++) {
    far = Math.max(far, (positions[i * 3] - cx) ** 2 + (positions[i * 3 + 1] - cy) ** 2 + (positions[i * 3 + 2] - cz) ** 2);
  }
  return { center: [cx, cy, cz], radius: Math.sqrt(far) };
}

/** A handful of nodes close together is framed as if it were this wide, so they do not fill the canvas. */
const MIN_FIT_RADIUS = 60;

/** How far back a camera stands to hold a sphere of this radius, on the narrower of its two angles. */
export function fitDistance(radius: number, fovDegrees: number, aspect: number): number {
  const vertical = (Math.min(170, Math.max(1, fovDegrees)) * Math.PI) / 360;
  const half = aspect > 0 && aspect < 1 ? Math.atan(Math.tan(vertical) * aspect) : vertical;
  return (Math.max(radius, MIN_FIT_RADIUS) / Math.sin(half)) * 1.15;
}

/**
 * Whether a frame writes the node buffers, and whether one more write is owed after it. A frame
 * inside a move owes the frame after the move its final state: frames can stop (a hidden window)
 * and come back past the move's end, where neither "moving" nor "dirty" would ask for it.
 */
export function frameWrite(moving: boolean, dirty: boolean, owed: boolean): { write: boolean; owed: boolean } {
  return { write: moving || dirty || owed, owed: moving };
}

/**
 * Whether the camera's one fit per scope visit is used up: nothing more is on its way and what
 * was placed is the layout of the view on screen. A layout of an earlier, smaller view still
 * frames the camera, but the fit stays owed to the layout that replaces it.
 */
export function fitSettled(state: { loading: boolean; busy: boolean; placed: string; current: string }): boolean {
  return !state.loading && !state.busy && state.placed === state.current;
}

type Vec = { x: number; y: number; z: number };

/** The nearest node sphere a ray meets (origin + t·direction, direction normalized), or -1. */
export function pickSphere(origin: Vec, direction: Vec, positions: ArrayLike<number>, radii: ArrayLike<number>, count: number): number {
  let best = -1;
  let nearest = Infinity;
  for (let i = 0; i < count; i++) {
    const [ox, oy, oz] = [positions[i * 3] - origin.x, positions[i * 3 + 1] - origin.y, positions[i * 3 + 2] - origin.z];
    const along = ox * direction.x + oy * direction.y + oz * direction.z;
    // A radius of 0 is a node not on screen yet.
    if (radii[i] <= 0 || along <= 0 || along >= nearest) continue;
    const off = ox * ox + oy * oy + oz * oz - along * along;
    if (off <= radii[i] * radii[i]) {
      best = i;
      nearest = along;
    }
  }
  return best;
}
