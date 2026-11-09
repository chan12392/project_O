# Third-party notices

## Orca

project_O's app UI follows Orca's design system. Copied or adapted from
[stablyai/orca](https://github.com/stablyai/orca) at commit `0646d26ff00289b86763acc52e42faeb73a94110`:

- `app/src/styles/orca-theme.css`: design tokens, copied verbatim from `src/renderer/src/assets/main.css`
- `app/src/ui/AgentStateDot.tsx`: adapted from `AgentStateDot.tsx` and `AgentWorkingSpinner.tsx`
- Layout and class recipes of the sidebar rows, tab strip, native chat column, composer and status bar, rebuilt for O's data
- `crates/o-core/src/agents/catalog.rs`, `crates/o-core/src/sys/resolve.rs`, `crates/o-core/src/agents/clean.rs`: agent commands, installation discovery and profile trust handling adapted from Orca's MIT-licensed GitHub source.

```
MIT License

Copyright (c) 2026 Lovecast Inc.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Kibo UI

The former `app/src/components/Kanban.tsx` adapted the column and card structure of
[Kibo UI's Kanban](https://github.com/shadcnblocks/kibo/blob/3d63cdb15b79d972e3dc38a10997987672f9b263/packages/kanban/index.tsx),
without its drag and drop. It was later moved to `app/src/features/work/Kanban.tsx`
and removed in project_O commit `c2be085`. Its original
[license](https://github.com/shadcnblocks/kibo/blob/3d63cdb15b79d972e3dc38a10997987672f9b263/license.md)
is retained here for the historical adaptation:

```
Copyright (c) 2023 — Present shadcnblocks

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

## Generated art

`app/public/office/*.png` (office background and four characters) were generated for this
project using Codex's built-in image generation tool, dispatched through O CLI (run
`20261101-151813-l32j`, 2026-11-01 UTC / 2026-11-02 Korea time). The recorded run says
the first generated character was used as a reference for the others. The available
record does not establish the complete set of inputs, so third-party input cannot be ruled out.

`docs/brand/icon.png` is AI-generated artwork made for this project. The current
`app/src-tauri/icons/` assets use the office lead character (commit `696f079`),
generated in the office artwork run above; earlier icons used the brand image.

`app/public/office/sheets/raw-*.png` and `raw-b-*.png` (action sheets) were generated the same way
from those characters (runs `20261101-161526-km7d`, `20261101-162328-9p0b`); `sheet-*.png` are
their aligned copies made by `scripts/align-sprite-sheets.py`.

`app/public/office/parts/raw-*.png` contains base bodies, outfits, hair and expression
sheets for composed characters. The previous version of this notice attributed their generation to runs
`20261101-164249-vdcc` and `20261101-165327-6n89`; those run records and their inputs
have not been independently verified here. The tracked `char-*.png` files are derived
from the raw sheets using `scripts/align-sprite-sheets.py` and
`scripts/build-character-parts.py`; `body-*.png` files are intermediate outputs.

## Rust crates

The Windows build uses crates from crates.io, including MIT, Apache-2.0 and MPL-2.0
components. The exact versions and their license texts are in
[`THIRD_PARTY_LICENSES.txt`](THIRD_PARTY_LICENSES.txt); source archives for the MPL
components are available from crates.io at the versioned URLs described there.
`cargo tree --target x86_64-pc-windows-msvc` lists the dependency graph.

## Fonts

- Geist (`@fontsource-variable/geist`), SIL Open Font License 1.1
- Pretendard (`pretendard`), SIL Open Font License 1.1

The fonts' copyright notices and full licenses are included in `THIRD_PARTY_LICENSES.txt`.
