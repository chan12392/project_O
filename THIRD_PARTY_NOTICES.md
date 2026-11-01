# Third-party notices

## Orca

project_O's app UI follows Orca's design system. Copied or adapted from
[stablyai/orca](https://github.com/stablyai/orca) at commit `0646d26ff00289b86763acc52e42faeb73a94110`:

- `app/src/styles/orca-theme.css`: design tokens, copied verbatim from `src/renderer/src/assets/main.css`
- `app/src/components/AgentStateDot.tsx`: adapted from `AgentStateDot.tsx` and `AgentWorkingSpinner.tsx`
- Layout and class recipes of the sidebar rows, tab strip, native chat column, composer and status bar, rebuilt for O's data

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

## Generated art

`app/public/office/*.png` (사무실 배경, 캐릭터 4종) were generated for this project on 2026-11-02
by Codex's built-in `image_gen` tool, dispatched through O CLI (run `20261101-151813-l32j`).
No third-party artwork or logos were used as input.

## Fonts

- Geist (`@fontsource-variable/geist`), SIL Open Font License 1.1
- Pretendard (`pretendard`), SIL Open Font License 1.1
