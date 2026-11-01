"""Aligns generated 4 x 4 character sheets into clean 256px-cell sheets.

Generated sheets rarely sit on an even grid: rows differ in height, sprites in
neighbouring rows can touch, and a raised hand can cross into the next cell.
So this cuts where the sheet is emptiest near each quarter line (not at the
quarter itself), keeps a cell's own sprite and loose bits (a sweat drop) but
drops slivers of a neighbour along the cell edge, scales every sheet of a set
by the same factor and keeps each row's own vertical motion (a jump stays a jump).

    python scripts/align-sprite-sheets.py <sheets dir> <raw prefix> <out prefix> [--scale S] <name> [<name> ...]
    e.g. python scripts/align-sprite-sheets.py app/public/office/sheets raw- sheet- claude codex agy lead

Pass the first set's printed scale with --scale when aligning another set, so a
character stays the same size whichever sheet it is playing from.
"""
import sys
from collections import deque
from pathlib import Path

from PIL import Image

CELL, GRID, FOOT = 256, 4, 8
ALPHA, POOL = 40, 2  # opacity threshold; component search runs on 2 x 2 blocks for speed
WINDOW = 0.12  # how far from a quarter line a cut may move, as a share of the sheet


def components(im):
    """Labels 8-connected opaque regions on a pooled grid; returns (labels, sizes, pooled size)."""
    alpha = im.getchannel("A").point(lambda a: 255 if a > ALPHA else 0)
    w, h = (im.width + POOL - 1) // POOL, (im.height + POOL - 1) // POOL
    pooled = alpha.resize((w, h), Image.Resampling.BOX).load()
    labels = [[0] * w for _ in range(h)]
    sizes = {}
    next_label = 0
    for y in range(h):
        for x in range(w):
            if pooled[x, y] == 0 or labels[y][x]:
                continue
            next_label += 1
            queue, count = deque([(x, y)]), 0
            labels[y][x] = next_label
            while queue:
                cx, cy = queue.popleft()
                count += 1
                for nx in (cx - 1, cx, cx + 1):
                    for ny in (cy - 1, cy, cy + 1):
                        if 0 <= nx < w and 0 <= ny < h and pooled[nx, ny] and not labels[ny][nx]:
                            labels[ny][nx] = next_label
                            queue.append((nx, ny))
            sizes[next_label] = count
    return labels, sizes, (w, h)


def emptiest(profile, length):
    """The three cut positions: near each quarter line, where the fewest opaque pixels lie."""
    cuts = []
    for k in range(1, GRID):
        expected = k * length // GRID
        lo, hi = int(expected - WINDOW * length), int(expected + WINDOW * length)
        cuts.append(min(range(lo, hi), key=lambda i: (profile[i], abs(i - expected))))
    return [0, *cuts, length]


def profile(im, axis):
    """Opaque pixel count per row (axis 1) or per column (axis 0)."""
    mask = im.getchannel("A").point(lambda a: 1 if a > ALPHA else 0)
    w, h = mask.size
    px = mask.load()
    if axis == 1:
        return [sum(px[x, y] for x in range(w)) for y in range(h)]
    return [sum(px[x, y] for y in range(h)) for x in range(w)]


def cells(im):
    """Yields (row, col, box) for the 16 cells, cutting along the emptiest lines."""
    ys = emptiest(profile(im, 1), im.height)
    for r in range(GRID):
        band = im.crop((0, ys[r], im.width, ys[r + 1]))
        xs = emptiest(profile(band, 0), im.width)
        for c in range(GRID):
            yield r, c, (xs[c], ys[r], xs[c + 1], ys[r + 1])


def cut(cell):
    """The cell's own sprite: its largest region plus loose bits, without a neighbour's slivers."""
    labels, sizes, (w, h) = components(cell)
    biggest = max(sizes, key=sizes.get)
    edge = {labels[y][x] for y in range(h) for x in (0, w - 1)} | {labels[y][x] for x in range(w) for y in (0, h - 1)}
    keep = {l for l in sizes if l == biggest or (l not in edge and sizes[l] >= sizes[biggest] * 0.005) or sizes[l] >= sizes[biggest] * 0.3}
    src = cell.load()
    out = Image.new("RGBA", cell.size)
    dst = out.load()
    for y in range(cell.height):
        row = labels[y // POOL]
        for x in range(cell.width):
            if row[x // POOL] in keep:
                dst[x, y] = src[x, y]
    box = out.getchannel("A").point(lambda a: 255 if a > ALPHA else 0).getbbox()
    return out.crop(box), box


def main():
    folder, raw_prefix, out_prefix, rest = Path(sys.argv[1]), sys.argv[2], sys.argv[3], sys.argv[4:]
    forced = None
    if rest[:1] == ["--scale"]:
        forced, rest = float(rest[1]), rest[2:]
    names = rest
    sheets = {}
    for name in names:
        im = Image.open(folder / f"{raw_prefix}{name}.png").convert("RGBA")
        cut_rows = [[] for _ in range(GRID)]
        for r, _c, box in cells(im):
            sprite, inner = cut(im.crop(box))
            cut_rows[r].append((sprite, box[1] + inner[3]))
        # Keep how far each frame's feet sit above its row's floor, so jumps and bobs survive.
        sheets[name] = [[(s, max(b for _, b in row) - bottom) for s, bottom in row] for row in cut_rows]
    tallest = max(s.height + lift for rows in sheets.values() for row in rows for s, lift in row)
    widest = max(s.width for rows in sheets.values() for row in rows for s, _ in row)
    fit = min((CELL - FOOT - 8) / tallest, (CELL - 8) / widest)
    if forced and forced > fit:
        raise SystemExit(f"--scale {forced} would not fit these frames; the largest that fits is {fit:.3f}")
    scale = forced or fit
    for name, rows in sheets.items():
        out = Image.new("RGBA", (CELL * GRID, CELL * GRID))
        for r, row in enumerate(rows):
            for c, (sprite, lift) in enumerate(row):
                small = sprite.resize((max(1, round(sprite.width * scale)), max(1, round(sprite.height * scale))), Image.Resampling.NEAREST)
                x = c * CELL + (CELL - small.width) // 2
                y = r * CELL + CELL - FOOT - round(lift * scale) - small.height
                out.paste(small, (x, y), small)
        out.save(folder / f"{out_prefix}{name}.png")
        print(f"{out_prefix}{name}.png  scale {scale:.3f}")


if __name__ == "__main__":
    main()
