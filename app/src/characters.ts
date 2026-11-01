/**
 * Composable characters: a bald, faceless body sheet with a hair piece and an
 * expression laid over each frame's head, drawn once per look into a canvas.
 * The parts and each frame's head box come from scripts/build-character-parts.py.
 */
const BASE = "/office/parts";

export type Body = "male-tee" | "male-hoodie" | "female-tee" | "female-cardigan";
export const BODIES: Body[] = ["male-tee", "male-hoodie", "female-tee", "female-cardigan"];

export interface Look {
  body: Body;
  /** Hair style index, 0 to 3, within the body's gender. */
  hair: number;
}

interface Parts {
  cell: number;
  rows: ("front" | "side" | "back" | "backdiag")[];
  expressions: string[];
  bodies: Record<Body, { gender: "male" | "female"; head: [number, number]; boxes: [number, number][] }>;
  hair: Record<string, { columns: Record<string, number>; rects: number[][]; offsets: number[][] }>;
  faces: Record<string, { rects: number[][]; offsets: number[][] }>;
}

/** The expression each of the 12 rows wears (move, act1, act2); back views have none. */
const ROW_FACE = ["neutral", "", "smile", "", "smile", "", "smile", "talk", "laugh", "think", "excited", "flustered"];

let parts: Promise<Parts> | undefined;
const images = new Map<string, Promise<HTMLImageElement>>();
const sheets = new Map<string, Promise<string>>();

function image(name: string): Promise<HTMLImageElement> {
  if (!images.has(name)) {
    images.set(
      name,
      new Promise((resolve, reject) => {
        const img = new Image();
        img.onload = () => resolve(img);
        img.onerror = () => reject(new Error(`${name}을(를) 불러오지 못했습니다`));
        img.src = `${BASE}/${name}`;
      }),
    );
  }
  return images.get(name)!;
}

/** Draws a look's 4 x 12 sheet and hands back an object URL for it, once per look. */
export function characterSheet(look: Look): Promise<string> {
  const key = `${look.body}/${look.hair}`;
  if (!sheets.has(key)) sheets.set(key, compose(look));
  return sheets.get(key)!;
}

async function compose(look: Look): Promise<string> {
  parts ??= fetch(`${BASE}/characters.json`).then((r) => r.json() as Promise<Parts>);
  const data = await parts;
  const body = data.bodies[look.body];
  const g = body.gender;
  const [bodyImg, maskImg, hairImg, faceImg] = await Promise.all([
    image(`char-body-${look.body}.png`),
    image(`char-mask-${look.body}.png`),
    image(`char-hair-${g}.png`),
    image(`char-face-${g}.png`),
  ]);
  const { cell } = data;
  const [w] = body.head;
  const out = document.createElement("canvas");
  out.width = bodyImg.width;
  out.height = bodyImg.height;
  const ctx = out.getContext("2d")!;
  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(bodyImg, 0, 0);

  // A face goes only on the head's own skin, so a mug or a hand in front of it stays in front.
  const face = document.createElement("canvas");
  face.width = face.height = cell;
  const fctx = face.getContext("2d")!;
  fctx.imageSmoothingEnabled = false;

  data.rows.forEach((facing, r) => {
    for (let c = 0; c < 4; c += 1) {
      const [hx, hy] = body.boxes[r * 4 + c];
      const x0 = c * cell;
      const y0 = r * cell;
      const expression = data.expressions.indexOf(ROW_FACE[r]);
      if (facing === "front" && expression >= 0) {
        const [fx, fy, fw, fh] = data.faces[g].rects[expression];
        const [ox, oy] = data.faces[g].offsets[expression];
        fctx.globalCompositeOperation = "source-over";
        fctx.clearRect(0, 0, cell, cell);
        fctx.drawImage(faceImg, fx, fy, fw, fh, hx + ox, hy + oy, fw, fh);
        fctx.globalCompositeOperation = "destination-in";
        fctx.drawImage(maskImg, x0, y0, cell, cell, 0, 0, cell, cell);
        ctx.drawImage(face, x0, y0);
      }
      const piece = look.hair * 4 + data.hair[g].columns[facing];
      const [px, py, pw, ph] = data.hair[g].rects[piece];
      const [dx, dy] = data.hair[g].offsets[piece];
      ctx.drawImage(hairImg, px, py, pw, ph, x0 + hx + Math.round(w / 2 - pw / 2) + dx, y0 + hy + dy, pw, ph);
    }
  });

  const blob = await new Promise<Blob | null>((resolve) => out.toBlob(resolve));
  if (!blob) throw new Error("캐릭터 시트를 그리지 못했습니다");
  return URL.createObjectURL(blob);
}

/** A steady look per card: the same card always gets the same person. */
export function lookFor(id: string): Look {
  let hash = 2166136261;
  for (const ch of id) hash = Math.imul(hash ^ ch.charCodeAt(0), 16777619) >>> 0;
  return { body: BODIES[hash % BODIES.length], hair: (hash >>> 4) % 4 };
}
