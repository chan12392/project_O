# Third-party notices

## Orca

project_O's app UI follows Orca's design system. Copied or adapted from
[stablyai/orca](https://github.com/stablyai/orca) at commit `0646d26ff00289b86763acc52e42faeb73a94110`:

- `app/src/styles/orca-theme.css`: design tokens, copied verbatim from `src/renderer/src/assets/main.css`
- `app/src/components/AgentStateDot.tsx`: adapted from `AgentStateDot.tsx` and `AgentWorkingSpinner.tsx`
- Layout and class recipes of the sidebar rows, tab strip, native chat column, composer and status bar, rebuilt for O's data
- `crates/o-core/src/agents/catalog.rs`, `crates/o-core/src/sys/resolve.rs`, `crates/o-core/src/agents/clean.rs`: 에이전트 명령·별칭 표, 설치 폴더 탐색과 프로필 신뢰 처리를 Orca 1.4.246 조사 결과에서 각색했다. 위 커밋은 저장소에서 확인한 별도 기준이며 설치본과 일치한다고 가정하지 않는다.

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

`app/src/components/Kanban.tsx` adapts the column and card structure of Kibo UI's Kanban
(https://github.com/haydenbleasel/kibo, `packages/kanban/index.tsx` at commit
3d63cdb15b79d972e3dc38a10997987672f9b263), without its drag and drop.

```
Copyright (c) 2023 — Present shadcnblocks

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

## Generated art

`app/public/office/*.png` (사무실 배경, 캐릭터 4종) were generated for this project on 2026-11-02
by Codex's built-in `image_gen` tool, dispatched through O CLI (run `20261101-151813-l32j`).
No third-party artwork or logos were used as input.

`app/public/office/sheets/raw-*.png` and `raw-b-*.png` (action sheets) were generated the same way
from those characters (runs `20261101-161526-km7d`, `20261101-162328-9p0b`); `sheet-*.png` are
their aligned copies made by `scripts/align-sprite-sheets.py`.

`app/public/office/parts/raw-*.png` (bald, faceless base bodies, outfits, hair and expression
sheets for composed characters) were generated the same way (runs `20261101-164249-vdcc`,
`20261101-165327-6n89`); `body-*.png` and `char-*.png` are derived from them by
`scripts/align-sprite-sheets.py` and `scripts/build-character-parts.py`.

## Rust crates

The `o` command (`crates/`) builds on crates from crates.io under permissive licenses:
serde, serde_json, regex, chrono, windows-sys and Tauri 2 for the desktop shell `project_O.exe` (MIT or Apache-2.0), rusqlite (MIT) with
SQLite compiled in (public domain). `cargo tree` lists the full set; their license texts
ship in each crate's source.

## Fonts

- Geist (`@fontsource-variable/geist`), SIL Open Font License 1.1
- Pretendard (`pretendard`), SIL Open Font License 1.1
