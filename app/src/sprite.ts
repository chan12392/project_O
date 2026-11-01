/**
 * The desk sprite: an agent seated at a desk, side view, 24 x 20 pixels.
 * Every key is a CSS class (`px-<key>`) whose fill is a token, so the
 * provider and the card's state recolour it without new art.
 *
 *   k ink outline · h hair · s skin · c shirt · l trousers · r chair
 *   d desk top · e desk legs · g screen · L screen marks · p copy paper
 */
export const WIDTH = 24;
export const HEIGHT = 20;

const BASE = [
  "........................",
  "....kkkk................",
  "...khhhhk...............",
  "...khhhhhk..............",
  "...khhsssk......kkkkkkk.",
  "...khsssk.......kgggggk.",
  "....kssk........kgLLLgk.",
  "....kkkk........kgggggk.",
  "...kcccck.......kgLLggk.",
  "..kcccccck......kkkkkkk.",
  "..kcccccck..........kk..",
  "rrkcccccck....dddddddddd",
  "rrkllllllllk..eeeeeeeeee",
  "rrkllllllllk...e......e.",
  "rr.kkkkkkklk...e......e.",
  "rr.......klk...e......e.",
  "rrrrrrr..klk...e......e.",
  "..r...r..kkkk..e......e.",
  "..r...r........e......e.",
  "........................",
];

type Pixel = [x: number, y: number, key: string];

const KEYBOARD: Pixel[] = [[16, 10, "k"], [17, 10, "k"], [18, 10, "k"]];
const ARM_REST: Pixel[] = [[8, 10, "c"], [9, 10, "c"], [10, 10, "s"]];
const ARM_TYPE_A: Pixel[] = [[8, 10, "c"], [9, 10, "c"], [10, 10, "c"], [11, 10, "c"], [12, 10, "s"], [13, 10, "s"]];
const ARM_TYPE_B: Pixel[] = [[8, 10, "c"], [9, 10, "c"], [10, 10, "c"], [11, 10, "c"], [12, 10, "c"], [13, 10, "c"], [14, 10, "s"], [15, 10, "s"]];
const PAPERS: Pixel[] = [[14, 9, "p"], [15, 9, "p"], [14, 10, "p"], [15, 10, "p"]];

export interface Rect {
  x: number;
  y: number;
  w: number;
  key: string;
}

/** Turns a grid plus overlays into horizontal runs, one rect per run. */
function rects(overlays: Pixel[]): Rect[] {
  const grid = BASE.map((row) => row.split(""));
  for (const [x, y, key] of overlays) grid[y][x] = key;
  const out: Rect[] = [];
  grid.forEach((row, y) => {
    let x = 0;
    while (x < row.length) {
      const key = row[x];
      let w = 1;
      while (x + w < row.length && row[x + w] === key) w += 1;
      if (key !== ".") out.push({ x, y, w, key });
      x += w;
    }
  });
  return out;
}

export type Pose = "rest" | "done";

/** Still poses, and the two typing frames that alternate while a card runs. */
export const POSES: Record<Pose, Rect[]> = {
  rest: rects([...KEYBOARD, ...ARM_REST]),
  done: rects([...KEYBOARD, ...ARM_REST, ...PAPERS]),
};
export const TYPING: [Rect[], Rect[]] = [rects([...KEYBOARD, ...ARM_TYPE_A]), rects([...KEYBOARD, ...ARM_TYPE_B])];
