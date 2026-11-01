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


def head_skin(cell, h, y):
    """The head's own skin: the biggest skin region above the bottom of a head box at y."""
    mask = skin(cell)
    mask[y + h :, :] = False
    return biggest_component(mask)


def locate_head(cell, w, h):
    """Top-left of the w x h box holding the most head skin, its top on the skull's top.

    The box with the most skin also takes in the neck, so it sits a few pixels below
    the skull by an amount that changes frame to frame; snapping it to the skull keeps
    hair and faces at the same height on every frame.
    """
    mask = skin(cell)
    opaque = np.nonzero(cell[..., 3] > 40)
    top, bottom = opaque[0].min(), opaque[0].max()
    mask[int(top + (bottom - top) * 0.62) :, :] = False  # hands and knees below the head don't count
    integral = np.pad(mask.astype(np.int32).cumsum(0).cumsum(1), ((1, 0), (1, 0)))
    sums = integral[h:, w:] - integral[:-h, w:] - integral[h:, :-w] + integral[:-h, :-w]
    best = sums.max()
    ys, xs = np.nonzero(sums >= best - 2)
    i = np.argmin(ys * 1000 + np.abs(xs + w / 2 - CELL / 2))  # the topmost of near-equal boxes
    x, y = int(xs[i]), int(ys[i])
    # The skull's top: the first skin row over the box's middle half (a hand at the side doesn't count).
    rows = np.nonzero(head_skin(cell, h, y)[:, x + w // 4 : x + 3 * w // 4].any(axis=1))[0]
    if len(rows):
        y = int(np.clip(rows.min(), y - h * 0.15, y + h * 0.15))
    return x, y


# The body frame each hair direction is fitted on: (row, column) in the 12-row stack.
FIT_FRAME = {"front": (0, 0), "side": (1, 0), "back": (3, 0), "backdiag": (5, 0)}
# Piece sizes tried around the style's HAIR_FIT, as factors of it; the nearest to 1 wins among equals.
FIT_SCALES = [0.92, 0.96, 1.0, 1.04, 1.08, 1.12, 1.16, 1.2, 1.25]


def placements(weight, piece):
    """Sum of weight under a piece mask for every top-left (y, x): result[y + ph, x + pw]."""
    ph, pw = piece.shape
    size = (weight.shape[0] + 2 * ph, weight.shape[1] + 2 * pw)
    padded = np.zeros(size)
    padded[ph : ph + weight.shape[0], pw : pw + weight.shape[1]] = weight
    return np.fft.irfft2(np.fft.rfft2(padded) * np.conj(np.fft.rfft2(piece.astype(float), s=size)), s=size)


def fit_hair(raw, factor, cell, box, head, facing, features):
    """Size and place a hair piece on its head box: most skull covered, least face hidden.

    Skull is the head's skin over the top half of the box. Covering the eyes and mouth
    (features, front views) costs far more than covering cheeks or the side of the face,
    which long hair may. Returns the scaled piece and [dx, dy] in pixels from the
    centred-on-the-head, top-aligned spot.
    """
    w, h = head
    hx, hy = box
    own = head_skin(cell, h, hy)
    # The skull with its outline, so hair doesn't leave a dark rim showing over it.
    rim = own.copy()
    for _ in range(2):
        rim |= np.roll(rim, -1, 0) | np.roll(rim, 1, 1) | np.roll(rim, -1, 1)
    rim &= cell[..., 3] > 40
    # Hair beside or above the head costs a little, so pieces don't grow past what covering needs.
    weight = np.zeros(own.shape)
    weight[: hy + h] = -0.1
    weight[: hy + int(h * 0.5)][rim[: hy + int(h * 0.5)]] = 1
    if facing == "side":  # the back of the head, behind the ear, is hair too
        weight[: hy + int(h * 0.62), : hx + int(w * 0.45)][rim[: hy + int(h * 0.62), : hx + int(w * 0.45)]] = 1
    face = np.zeros_like(own)
    if facing == "front":
        face[hy + int(h * 0.42) : hy + h, hx + int(w * 0.18) : hx + int(w * 0.82)] = True
    elif facing == "side":  # faces right
        face[hy + int(h * 0.42) : hy + h, hx + int(w * 0.55) : hx + w] = True
    weight[face & own] = -1
    weight[features & own] = -6
    skull = weight.clip(0).sum()
    candidates = []
    for s in FIT_SCALES:
        piece = scaled(raw, factor * s)
        hair = np.array(piece)[..., 3] > 40
        ph, pw = hair.shape
        score = placements(weight, hair)
        dys, dxs = np.mgrid[-int(ph * 0.6) : int(h * 0.25) + 1, -int(w * 0.3) : int(w * 0.3) + 1]
        values = np.rint(score[hy + dys + ph, hx + round(w / 2 - pw / 2) + dxs + pw])
        candidates += [(v, abs(s - 1), abs(dx) + abs(dy), [int(dx), int(dy)], piece) for v, dx, dy in zip(values.ravel(), dxs.ravel(), dys.ravel())]
    best = max(c[0] for c in candidates)
    # Among placements within half a percent of the skull of the best: the nearest size, then the smallest move.
    pick = min((c for c in candidates if c[0] >= best - skull * 0.005), key=lambda c: c[1:3])
    return pick[4], pick[3]


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

        # Where the neutral expression draws eyes and mouth on the front fit frame, a pixel wider:
        # hair keeps off it. (Other expressions add brows, which bangs may cover.)
        base = data["bodies"][f"{gender}-tee"]
        fx, fy = base["boxes"][FIT_FRAME["front"][0] * GRID + FIT_FRAME["front"][1]]
        (ox, oy), a = offsets[EXPRESSIONS.index("neutral")], np.array(features[EXPRESSIONS.index("neutral")])[..., 3] > 40
        drawn = np.zeros((CELL, CELL), dtype=bool)
        drawn[fy + oy : fy + oy + a.shape[0], fx + ox : fx + ox + a.shape[1]] = a
        drawn |= np.roll(drawn, 1, 0) | np.roll(drawn, -1, 0) | np.roll(drawn, 1, 1) | np.roll(drawn, -1, 1)

        # The generator draws hair at its own size, so each style starts from its front piece
        # spanning the head plus the style's volume (HAIR_FIT of the head's width); each piece
        # is then sized and placed on its own direction's fit frame.
        hair = pieces(Image.open(folder / f"raw-hair-{gender}.png").convert("RGBA"))
        flat, offsets = [], []
        for r in range(GRID):
            factor = heads_w * HAIR_FIT[gender][r] / hair[r][HAIR_COLUMN["front"]].width
            for d, c in HAIR_COLUMN.items():
                fr, fc = FIT_FRAME[d]
                piece, offset = fit_hair(hair[r][c], factor, base_cells[fr][fc], base["boxes"][fr * GRID + fc], base["head"], d, drawn if d == "front" else np.zeros_like(drawn))
                flat.append(piece)
                offsets.append(offset)
        rects = pack(flat, folder / f"char-hair-{gender}.png")
        data["hair"][gender] = {"styles": HAIR_STYLES[gender], "columns": HAIR_COLUMN, "rects": rects, "offsets": offsets}
        print(f"char-hair-{gender}.png")
    (folder / "characters.json").write_text(json.dumps(data, separators=(",", ":")) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
