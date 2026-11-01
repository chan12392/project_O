"""Prepares composable character parts from the aligned body sheets and the raw layer sheets.

A character is a body sheet (bald, faceless; one per gender and outfit) with a hair
piece and an expression laid over its head, frame by frame. Generated layers never
line up pixel for pixel, so nothing here relies on that: every body frame gets its
own head box (found from skin pixels), hair and faces are cut to their own pieces,
and the app places pieces on each frame's head box at runtime (app/src/characters.ts).

    python scripts/align-sprite-sheets.py app/public/office/parts raw- body- --scale 0.725 <bodies>
    python scripts/build-character-parts.py app/public/office/parts

Writes, next to the inputs:
  char-body-<gender>-<outfit>.png   move, act1 and act2 stacked: 4 x 12 cells of 256px
  char-mask-<gender>-<outfit>.png   the head's skin in each frame (where a face may be drawn)
  char-hair-<gender>.png            hair pieces, 4 styles x 4 directions, scaled to the body
  char-face-<gender>.png            16 expressions (features only), scaled to the body
  characters.json                   head boxes per frame, piece rectangles, row meanings
"""
import importlib.util
import json
import sys
from collections import deque
from pathlib import Path

import numpy as np
from PIL import Image

CELL, GRID = 256, 4
SHEETS = ["move", "act1", "act2"]
OUTFITS = {"male": ["base", "outfit-hoodie"], "female": ["base", "outfit-cardigan"]}
OUTFIT_NAME = {"base": "tee", "outfit-hoodie": "hoodie", "outfit-cardigan": "cardigan"}
# Which way the head faces in each of the 12 rows (move, act1, act2).
FACING = ["front", "side", "front", "back", "front", "backdiag", "front", "front", "front", "front", "front", "front"]
HAIR_COLUMN = {"front": 0, "side": 1, "back": 2, "backdiag": 3}
HAIR_STYLES = {"male": ["black-short", "brown-twoblock", "blond-curly", "gray-parted"], "female": ["black-long", "brown-bob", "orange-ponytail", "purple-bun"]}
# Front hair piece width as a share of the head's (ears included), per style; tuned by eye.
HAIR_FIT = {"male": [1.08, 1.08, 1.2, 1.06], "female": [1.3, 1.22, 1.3, 1.3]}
EXPRESSIONS = ["neutral", "smile", "grin", "wink", "focus", "think", "sleepy", "yawn", "surprised", "flustered", "sad", "angry", "proud", "excited", "talk", "laugh"]

spec = importlib.util.spec_from_file_location("align", Path(__file__).with_name("align-sprite-sheets.py"))
align = importlib.util.module_from_spec(spec)
spec.loader.exec_module(align)


def skin(a):
    """Bald heads, hands and faces: bright, warm, red above blue. Grey, cream and brown clothes are not."""
    r, g, b, al = (a[..., i].astype(int) for i in range(4))
    return (al > 128) & (r > 150) & (r - b > 55) & (r >= g)


def biggest_component(mask):
    """The largest 4-connected region of a boolean mask, as a mask."""
    h, w = mask.shape
    seen = np.zeros_like(mask)
    best, best_size = None, 0
    for y0, x0 in zip(*np.nonzero(mask)):
        if seen[y0, x0]:
            continue
        queue, pixels = deque([(y0, x0)]), []
        seen[y0, x0] = True
        while queue:
            y, x = queue.popleft()
            pixels.append((y, x))
            for ny, nx in ((y - 1, x), (y + 1, x), (y, x - 1), (y, x + 1)):
                if 0 <= ny < h and 0 <= nx < w and mask[ny, nx] and not seen[ny, nx]:
                    seen[ny, nx] = True
                    queue.append((ny, nx))
        if len(pixels) > best_size:
            best, best_size = pixels, len(pixels)
    out = np.zeros_like(mask)
    if best:
        ys, xs = zip(*best)
        out[list(ys), list(xs)] = True
    return out


def head_width(cell):
    """Width of a front-facing head: the widest skin row of the biggest skin region."""
    head = biggest_component(skin(cell))
    return int(head.sum(axis=1).max())


def locate_head(cell, w, h):
    """Top-left of the w x h box holding the most head skin, searched in the frame's upper part."""
    mask = skin(cell)
    opaque = np.nonzero(cell[..., 3] > 40)
    top, bottom = opaque[0].min(), opaque[0].max()
    mask[int(top + (bottom - top) * 0.62) :, :] = False  # hands and knees below the head don't count
    integral = np.pad(mask.astype(np.int32).cumsum(0).cumsum(1), ((1, 0), (1, 0)))
    sums = integral[h:, w:] - integral[:-h, w:] - integral[h:, :-w] + integral[:-h, :-w]
    best = sums.max()
    ys, xs = np.nonzero(sums >= best - 2)
    i = np.argmin(ys * 1000 + np.abs(xs + w / 2 - CELL / 2))  # the topmost of near-equal boxes
    return int(xs[i]), int(ys[i])


# The body frame each hair direction is fitted on: (row, column) in the 12-row stack.
FIT_FRAME = {"front": (0, 0), "side": (1, 0), "back": (3, 0), "backdiag": (5, 0)}


def fit_hair(piece, cell, box, head, facing):
    """Where a hair piece sits on its head box: the offset covering the most skull and the least face.

    Returns [dx, dy] in pixels from the centred-on-the-head, top-aligned spot.
    """
    w, h = head
    hx, hy = box
    hair = piece[..., 3] > 40
    ph, pw = hair.shape
    head_skin = skin(cell)
    skull = np.zeros_like(head_skin)
    skull[hy : hy + int(h * 0.5), hx : hx + w] = True
    skull &= head_skin
    face = np.zeros_like(head_skin)
    if facing == "front":
        face[hy + int(h * 0.42) : hy + h, hx + int(w * 0.18) : hx + int(w * 0.82)] = True
        face &= head_skin
    best, best_key = [0, 0], None
    for dy in range(-int(h * 0.4), int(h * 0.25) + 1):
        for dx in range(-int(w * 0.3), int(w * 0.3) + 1):
            x, y = hx + round(w / 2 - pw / 2) + dx, hy + dy
            x0, y0, x1, y1 = max(0, x), max(0, y), min(CELL, x + pw), min(CELL, y + ph)
            if x1 <= x0 or y1 <= y0:
                continue
            placed = hair[y0 - y : y1 - y, x0 - x : x1 - x]
            covered = int((placed & skull[y0:y1, x0:x1]).sum())
            masked = int((placed & face[y0:y1, x0:x1]).sum())
            # Most skull, least face; among equals, the smallest move.
            key = (covered - 2 * masked, -(abs(dx) + abs(dy)))
            if best_key is None or key > best_key:
                best, best_key = [dx, dy], key
    return best


def pieces(sheet):
    """The 16 pieces of a raw 4 x 4 layer sheet, row by row, each cut to its own pixels."""
    out = [[None] * GRID for _ in range(GRID)]
    for r, c, box in align.cells(sheet):
        out[r][c], _ = align.cut(sheet.crop(box))
    return out


def pack(images, path):
    """Packs pieces into one row-major atlas; returns their [x, y, w, h] rectangles."""
    width = sum(im.width for im in images) + len(images)
    atlas = Image.new("RGBA", (width, max(im.height for im in images)))
    rects, x = [], 0
    for im in images:
        atlas.paste(im, (x, 0))
        rects.append([x, 0, im.width, im.height])
        x += im.width + 1
    atlas.save(path)
    return rects


def scaled(im, factor):
    return im.resize((max(1, round(im.width * factor)), max(1, round(im.height * factor))), Image.Resampling.NEAREST)


def main():
    folder = Path(sys.argv[1])
    data = {"cell": CELL, "rows": FACING, "expressions": EXPRESSIONS, "bodies": {}, "hair": {}, "faces": {}}
    for gender, outfits in OUTFITS.items():
        raw_front = np.array(align.cut(next(Image.open(folder / f"raw-base-{gender}-move.png").convert("RGBA").crop(b) for _r, _c, b in align.cells(Image.open(folder / f"raw-base-{gender}-move.png").convert("RGBA"))))[0])
        raw_w = head_width(raw_front)
        heads_w = None
        for outfit in outfits:
            stacked = np.concatenate([np.array(Image.open(folder / f"body-{outfit}-{gender}-{s}.png").convert("RGBA")) for s in SHEETS])
            cells = [[stacked[r * CELL : (r + 1) * CELL, c * CELL : (c + 1) * CELL] for c in range(GRID)] for r in range(GRID * 3)]
            w = int(np.median([head_width(cells[0][c]) for c in range(GRID)]))
            h = round(w * 0.9)
            heads_w = heads_w or w
            if outfit == "base":
                base_cells = cells
            boxes, mask = [], np.zeros(stacked.shape[:2], dtype=np.uint8)
            for r in range(GRID * 3):
                for c in range(GRID):
                    x, y = locate_head(cells[r][c], w, h)
                    boxes.append([x, y])
                    region = skin(cells[r][c])[y : y + h, x : x + w]
                    mask[r * CELL + y : r * CELL + y + h, c * CELL + x : c * CELL + x + w] = region * 255
            key = f"{gender}-{OUTFIT_NAME[outfit]}"
            Image.fromarray(stacked).save(folder / f"char-body-{key}.png")
            alpha = Image.fromarray(mask)
            Image.merge("RGBA", (alpha, alpha, alpha, alpha)).save(folder / f"char-mask-{key}.png")
            data["bodies"][key] = {"gender": gender, "outfit": OUTFIT_NAME[outfit], "head": [w, h], "boxes": boxes}
            print(f"char-body-{key}.png  head {w}x{h}")

        # The generator draws hair at its own size, so each style is fitted by its front piece:
        # that piece spans the head plus the style's volume (HAIR_FIT of the head's width).
        hair = pieces(Image.open(folder / f"raw-hair-{gender}.png").convert("RGBA"))
        flat = []
        for r in range(GRID):
            factor = heads_w * HAIR_FIT[gender][r] / hair[r][HAIR_COLUMN["front"]].width
            flat += [scaled(hair[r][c], factor) for c in range(GRID)]
        rects = pack(flat, folder / f"char-hair-{gender}.png")
        base = data["bodies"][f"{gender}-tee"]
        offsets = [fit_hair(np.array(piece), base_cells[FIT_FRAME[d][0]][FIT_FRAME[d][1]], base["boxes"][FIT_FRAME[d][0] * GRID + FIT_FRAME[d][1]], base["head"], d) for piece, d in zip(flat, [d for _ in range(GRID) for d in HAIR_COLUMN])]
        data["hair"][gender] = {"styles": HAIR_STYLES[gender], "columns": HAIR_COLUMN, "rects": rects, "offsets": offsets}
        print(f"char-hair-{gender}.png  (raw head {raw_w}px)")

        faces = pieces(Image.open(folder / "raw-face.png").convert("RGBA"))
        features, offsets = [], []
        for r in range(GRID):
            for c in range(GRID):
                cell = np.array(faces[r][c])
                head = biggest_component(skin(cell))
                ys, xs = np.nonzero(head)
                fw = int(head.sum(axis=1).max())
                left, top = xs.min(), ys.min()
                bw, bh = xs.max() - left + 1, ys.max() - top + 1
                # Features: what isn't skin inside the face, away from the outline and the ears.
                keep = np.zeros(cell.shape[:2], dtype=bool)
                keep[top + int(bh * 0.22) : top + int(bh * 0.9), left + int(bw * 0.17) : left + int(bw * 0.83)] = True
                keep &= ~skin(cell) & (cell[..., 3] > 40)
                layer = cell.copy()
                layer[~keep] = 0
                f = heads_w / fw
                im = Image.fromarray(layer).crop(Image.fromarray(layer).getchannel("A").getbbox())
                fx, fy = np.nonzero(keep.T)[0].min(), np.nonzero(keep)[0].min()
                features.append(scaled(im, f))
                offsets.append([round((fx - left) * f), round((fy - top) * f)])
        rects = pack(features, folder / f"char-face-{gender}.png")
        data["faces"][gender] = {"rects": rects, "offsets": offsets}
    (folder / "characters.json").write_text(json.dumps(data, separators=(",", ":")) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
