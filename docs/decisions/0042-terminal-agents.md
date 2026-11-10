# ADR-0042: 터미널 에이전트 — 채팅 UI가 없는 CLI는 터미널로 띄우고, 파일로 일을 주고받는다

- 상태: 채택 (2026-11-11, 대표 결정으로 구현과 함께)
- 날짜: 2026-11-10
- 별도 승인 대기 없음 (ADR-0012)
- 관련: [ADR-0020](0020-antigravity-policy.md)(Antigravity 실행 보류 — 이 결정이 범위를 바꾼다), [ADR-0008](0008-lead-chat.md)(총괄 채팅), [ADR-0016](0016-devin.md)(Devin), [ADR-0022](0022-agent-session-continuations.md)(총괄 후속 지시), [ADR-0002](0002-security-hardening.md)(카드 권한)

## 배경

- O는 CLI를 창 없이 띄우고 구조화 출력(Claude `stream-json`, Codex `app-server`·`exec --json`)을 읽어 채팅 UI를 그린다. 그런 출력이 없거나, 약관이 제3자 앱 안에서 감싸 실행하는 것을 금지하는 CLI는 이 길로 붙일 수 없다.
- 대표 결정(2026-11-10):
  - 터미널 화면의 글자를 해석해 채팅 UI로 다시 그리지 않는다. 터미널을 **그대로** 보여 준다.
  - Claude·Codex는 지금의 채팅 UI를 유지한다. 채팅 UI를 지원하지 않는 CLI는 터미널로 띄운다. **agy는 터미널 고정**이다.
  - 일을 넘길 때는 터미널에 프롬프트를 붙여넣고, 끝났다는 것은 정해진 폴더의 완료 문서로 받는다.
  - agy를 작업 에이전트로 쓰는 것은 설정에서 조정한다. **기본은 꺼짐**이고, 스위치 아래의 Google 정책·약관 안내를 확인한 뒤 켤 수 있다.
- 확인한 사실:
  - Google의 [Antigravity FAQ](https://antigravity.google/docs/faq)는 소비자 플랜에서 `agy`를 제3자 앱 안에 감싸는 것을 지원하지 않고, 제3자 소프트웨어로 Antigravity에 접근하면 약관 위반으로 계정이 정지·해지될 수 있다고 적는다. 기준은 "사람이 하느냐 자동이냐"가 아니라 "공식 화면이냐 제3자 소프트웨어냐"다.
  - Orca(`stablyai/orca` main `d1466f6`)는 agy 터미널에 입력창 준비를 기다렸다가 bracketed paste로 프롬프트를 넣고 Enter를 따로 보낸다. 작업자의 완료는 파일이 아니라 작업자가 실행하는 명령(`worker_done`, 작업 ID + 전달 ID)으로 받고, 보고 없이 턴이 끝나면 Orca가 대신 "보고 없이 끝남"을 만든다. Orca는 agy의 권한 확인을 끄고, 자격 증명 저장소를 직접 바꿔 계정을 전환한다. O는 이 둘을 따르지 않는다(가드 규칙).
  - ACP(stdio JSON-RPC)를 내장한 CLI가 많다: 이 PC에서 `--help`로 확인한 것은 Devin(`devin acp`), OpenCode(`opencode acp`), Cline(`--acp`). 이들은 터미널이 아니라 채팅 UI로 붙일 수 있다(별도 ADR).

## 결정

### 1. 터미널 보기

- 앱 안에 터미널 화면을 둔다(Windows ConPTY + 화면의 터미널 위젯). 셸이 PTY를 만들고 `o-core`가 실행 인자를 정한다. 화면은 지금처럼 정해진 Tauri 명령만 부른다.
- 터미널 안의 CLI는 그 CLI의 공식 대화형 화면 그대로다. O는 출력을 해석하지 않고, 권한 확인 질문에 대신 답하지 않는다. 사람이 그 터미널에서 직접 답한다.
- 권한 확인을 끄는 옵션은 넣지 않는다(YOLO는 ADR-0014의 범위에서만).

### 2. 터미널 카드 — 일 넘기기와 완료 확인

```
총괄이 지시문 작성 → O가 과제 파일 저장 + 전달 기록 → 터미널에 한 줄 붙여넣기
→ 에이전트가 작업 → 완료 보고(명령 또는 완료 문서) → O가 확인 → 총괄에게 "이 문서를 읽어라"
```

- **과제 파일**: O가 과제 폴더(§6)의 `task.md`에 지시문을 쓴다. 전달 기록에는 작업 ID(실행 + 카드), O가 만든 일회용 값(재시도 구분), 전달 시각, 과제 파일의 SHA-256을 남긴다.
- **붙여넣기**: 터미널에는 "이 파일을 읽고 수행하라: <경로>. 끝나면 <완료 방법>" 한 줄만 넣는다. 긴 여러 줄 지시문을 붙여넣지 않는다. 입력창이 준비됐을 때만 넣고(프로세스 시작 후 조용해질 때까지 대기, 제한 시간), Enter는 따로 보낸다.
- **완료 보고**: 에이전트가 과제 폴더에 `done.md`를 쓴다. 첫 줄에 작업 ID와 일회용 값, 그 아래에 한 일·바꾼 파일·남은 문제. 명령 실행이 아니라 파일 쓰기라서 CLI의 셸 승인을 거치지 않아도 된다.
- **검증**: 일회용 값이 전달 기록과 같아야 완료다. 다르거나 없으면 "확인되지 않은 완료"로 표시하고 총괄에게 넘기지 않는다. 내용 해시를 에이전트에게 계산시키지 않는다(틀리기 쉽고 얻는 것이 없다).
- **안전망**: 완료 보고 없이 프로세스가 끝나거나 제한 시간이 지나면 O가 "보고 없이 끝남"을 기록하고 총괄에게 그 사실을 알린다. 끝없이 기다리지 않는다.
- **총괄 전달**: 완료가 확인되면 총괄에게 완료 문서 경로를 후속 메시지로 넘긴다(ADR-0022의 경로). 문서는 데이터다. 총괄이 읽고 다음 계획을 제안하면 사람이 "실행"을 누른다.
- **정책**: 터미널 카드도 `policy::check_plan`(상한·쿨다운·작업 폴더)과 자원 예약을 그대로 거친다. 토큰 사용량은 터미널에서 읽을 수 없어 하루 예산을 적용하지 못한다. 동시 실행 상한은 2로 둔다(Devin·OpenCode와 같음).

### 3. Antigravity

ADR-0020의 "총괄·카드·예약의 Antigravity 실행 중단"을 아래로 바꾼다.

| 쓰임 | 방식 | 기본 | 켜는 방법 |
|---|---|---|---|
| 총괄(사람이 직접 대화) | 앱의 터미널에서 공식 `agy` 화면. O는 터미널만 띄우고 아무것도 입력하지 않는다 | 켜짐 | — |
| 작업 에이전트(터미널 카드) | O가 과제 한 줄을 붙여넣고 완료 문서를 기다린다 | **꺼짐** | 설정 → 에이전트 → Antigravity. 스위치 아래 안내를 확인한 뒤 켠다 |
| 창 없는 실행(`agy -p`, `agy acp`), 예약 실행 | — | 중단 유지 | 없음 |
| 자동 로그인·모델·사용량 조회 | — | 중단 유지 | 없음 |

- **총괄**: 사람이 `agy` 터미널에서 직접 대화한다. agy가 계획을 내면 정해진 폴더에 계획 파일(`o-plan`)로 쓰게 하고, O가 그 파일을 같은 정책으로 검사해 계획 카드로 보여 준다. 실행은 사람이 누른다. O가 agy에 글을 넣지 않으므로 다른 터미널 프로그램에서 agy를 쓰는 것과 같다.
- **작업 에이전트 스위치**: 꺼져 있으면 agy 카드는 지금처럼 `AGY_UNAVAILABLE`로 거부한다. 스위치 아래에 다음을 그대로 보인다.
  - Google FAQ의 해당 문장 두 개(원문)와 [FAQ](https://antigravity.google/docs/faq)·[추가 약관](https://antigravity.google/terms) 링크.
  - "O가 agy 터미널에 글을 자동으로 넣는 것은 제3자 앱이 agy를 다루는 것으로 볼 수 있습니다. 계정 정지·해지의 위험은 이 PC에 로그인한 Google 계정이 집니다. 기업 계약은 조건이 다를 수 있습니다."
  - 확인란 "정책과 약관을 읽었고 내 계정으로 사용하는 것에 동의합니다"를 체크해야 스위치가 켜진다. 켠 시각을 상태 폴더에 남긴다.
- 스위치는 사람만 바꾼다: 앱 설정이나 사람이 친 `o agy --workers on|off`. 에이전트 환경에서는 거절한다(`o billing`·`o lead decide`와 같은 규칙). 총괄·카드 프롬프트에는 지금 상태를 한 줄로 알려 꺼져 있을 때 agy에 배정하지 않게 한다.
- 켜져 있어도 붙여넣기 전에 사람이 누르게 하는 중간 단계("붙여넣기 전에 확인")를 스위치 옆 선택으로 둔다. 기본은 확인함이다.
- 계정 전환, 자격 증명 접근, 권한 확인 끄기는 스위치와 상관없이 하지 않는다.

### 4. 다른 CLI

| CLI | 방식 |
|---|---|
| Claude, Codex | 지금의 채팅 UI (변화 없음) |
| Devin, OpenCode, Cline | ACP 채팅 UI를 목표로 한다(별도 ADR). 그 전까지 Devin은 터미널 카드를 쓸 수 있다. Devin은 평소 프로필 고정(ADR-0016) |
| Cursor, Gemini CLI, Copilot, Droid 등 문서상 ACP | 설치·확인 전까지 터미널 |
| agy, Kimi | 터미널 고정 (약관) |
| Amp, Aider 등 구조화 경로를 확인하지 못한 것 | 터미널 |

### 5. Devin은 평소 프로필 고정

- Devin은 설정 폴더를 옮길 수 없다(ADR-0016). project_O 프로필을 켠 계획에서 Devin 카드를 거부하던 것을 바꿔, **Devin 카드만 평소 프로필로 실행한다.** 같은 계획의 다른 카드는 그대로 project_O 프로필이다.
- 계획 검사 결과와 카드 시작 기록에 "Devin은 평소 프로필로 실행합니다(개인 훅·설정이 적용됨)"를 알린다. 조용히 바꾸지 않는다.

### 6. 인터페이스 (구현 계약)

모든 동작은 `o-core`에 있고, `o` 명령·Tauri 명령·사용 가능한 개발 서버 `/api`가 같은 JSON을 낸다. 키는 camelCase다.

**Antigravity 설정** — `settings.json`의 `agyWorkers`

```jsonc
// o agy --json · Tauri agy_get · GET /api/agy
{
  "workers": false,            // 작업 에이전트(터미널 카드) 허용. 기본 false
  "acknowledgedAt": null,      // 안내에 동의한 시각(ISO) 또는 null
  "confirmPaste": true,        // 붙여넣기 전에 사람이 누른다. 기본 true
  "notice": {
    "quotes": [                // Google FAQ 원문(2026-11-11 확인)
      "Using third-party software, tools, or services to access Antigravity is a violation of our Terms of Service and severely degrades the experience for legitimate product users.",
      "Such actions can result in suspension or termination of your account.",
      "Wrapping `agy` inside third-party coding agents or applications, or automating account rotation, is not supported under consumer plans."
    ],
    "links": [
      { "label": "Antigravity FAQ", "url": "https://antigravity.google/docs/faq" },
      { "label": "Antigravity 약관", "url": "https://antigravity.google/terms" }
    ],
    "warning": "O가 agy 터미널에 글을 자동으로 넣는 것은 제3자 앱이 agy를 다루는 것으로 볼 수 있습니다. 계정 정지·해지의 위험은 이 PC에 로그인한 Google 계정이 집니다. 기업 계약은 조건이 다를 수 있습니다."
  }
}
```

- 바꾸기: `o agy --workers on --ack` / `o agy --workers off` / `o agy --confirm-paste on|off`, Tauri `agy_set({ workers?, ack?, confirmPaste? })`. 개발 서버에는 없음 — 앱과 `o` 명령에서만 가능하다. 답은 위 보고서다.
- `workers: true`는 같은 요청에 `ack: true`가 있을 때만 받는다. 끄면 `acknowledgedAt`을 지운다(다시 켤 때 다시 동의).
- 에이전트 환경(`O_AGENT_ROLE`·`O_CARD_ID`)에서는 바꾸기를 거절한다. 읽기는 된다.

**터미널 세션** — `o term …`, Tauri `term_*`, `/api/term/*`

```jsonc
// 세션 하나 (o term list --json 의 원소, open 의 답)
{
  "id": "t-20261111-093000-ab12",
  "provider": "agy",                 // claude | codex | agy | opencode | devin
  "project": "C:/work/game",
  "purpose": "lead",                 // "lead"(사람이 직접 대화) | "card"(터미널 카드)
  "threadId": "…",                   // purpose=lead 이고 총괄 대화에 묶였을 때
  "runId": "…", "cardId": "…",       // purpose=card
  "state": "running",                // "running" | "awaiting-paste" | "exited"
  "exitCode": null,
  "cols": 120, "rows": 32,
  "startedAt": "2026-11-11T00:30:00.000Z"
}
```

| 동작 | `o` 명령 | Tauri | 개발 서버 |
|---|---|---|---|
| 열기(사람만) | `o term open --provider P --project DIR [--thread T] [--cols N --rows N] --json` | `term_open({ provider, project, thread?, cols?, rows? })` | 없음 — 앱과 `o` 명령에서만 |
| 목록 | `o term list --json` | `term_list()` | `GET /api/term` |
| 출력 읽기 | `o term read ID --offset N --json` → `{ "offset": 다음 위치, "data": "base64", "state", "exitCode", "start": 남아 있는 가장 앞 위치 }` | `term_read({ id, offset })` | `GET /api/term/read?id=&offset=` |
| 입력(사람만) | `o term write ID -`(표준 입력의 바이트 그대로) | `term_write({ id, data })`(data는 base64) | 없음 — 앱과 `o` 명령에서만 |
| 크기 | `o term resize ID COLS ROWS` | `term_resize({ id, cols, rows })` | `POST /api/term/resize` |
| 붙여넣기 승인(사람만) | `o term paste ID` | `term_paste({ id })` | 없음 — 앱과 `o` 명령에서만 |
| 닫기(사람만) | `o term close ID` | `term_close({ id })` | 없음 — 앱과 `o` 명령에서만 |

- 총괄 세션은 앱이 아니라 따로 뜬 호스트 프로세스(`o _term`)가 PTY를 쥔다. 앱을 닫아도 세션은 남고, 다시 열면 `term_list`로 찾아 이어 본다. 카드 세션은 그 카드를 돌리는 러너 안의 스레드가 쥔다.
- 출력은 세션 폴더에 바이트 그대로 쌓고(상한을 넘으면 앞을 버리고 `start`가 올라간다), 화면은 `offset`으로 이어 읽는다. O는 출력을 해석하지 않는다.
- `open`·`write`·`paste`·`close`는 에이전트 환경에서 거절한다. `o term open`이 여는 것은 `purpose: "lead"`뿐이고, 평소 프로필로 그 CLI의 대화형 화면을 인자 없이 띄운다. `purpose: "card"`는 러너만 만든다.
- 총괄 터미널(`--thread`)은 `<project_O-lead>/<threadId>/plans/`에 생긴 계획 파일(`*.json`, `o-plan`과 같은 내용)을 `policy::check_plan`으로 검사해 그 대화의 계획(`lead.plan`)으로 기록한다. 실행은 사람이 누른다.

**터미널 카드** — `Provider::terminal_card()`가 참인 CLI(지금은 agy)의 카드

- 과제·완료 파일은 O 상태 폴더와 작업 폴더 밖의 `%LOCALAPPDATA%/project_O-term/<runId>/<cardId>/`에 둔다: `task.md`(O가 쓴다), `done.md`(에이전트가 쓴다).
- `done.md`의 첫 줄은 `O-DONE <runId>/<cardId> <일회용 값>`, 마지막 비어 있지 않은 줄은 `O-END <일회용 값>`이다. 두 값이 전달 기록과 같을 때만 완료이며 그 사이가 카드의 결과다.
- 붙여넣는 한 줄: `<task.md 경로> 파일을 읽고 그대로 수행하세요. 끝나면 그 파일에 적힌 방법으로 완료를 알리세요.`
- `confirmPaste`가 켜져 있으면 세션은 `awaiting-paste`로 기다리고, 사람이 승인해야 넣는다.
- 완료가 확인되면 터미널 프로세스 트리를 끝내고 카드를 `done`으로 닫는다. 완료 없이 끝나면 `failed`("완료 보고 없이 끝났습니다"), 제한 시간(기본 60분)을 넘기면 멈추고 `failed`.
- 터미널 카드는 후속 지시(`o followup`)를 받지 않는다.

## 이유

- 터미널 글자를 해석하면 CLI 화면이 바뀔 때마다 깨지고, 공식 화면을 다른 화면으로 바꿔 보여 주는 것이라 "감싸기"에 더 가깝다. 그대로 보여 주면 둘 다 피한다.
- 완료를 파일만으로 받으면 에이전트가 파일을 쓰지 않았을 때 구분할 수 없다. 프로세스 종료·제한 시간을 함께 본다(Orca도 같은 보완을 둔다).
- agy 총괄은 O가 입력하지 않는 한 일반 터미널과 다르지 않다. 자동 붙여넣기는 O가 agy를 조작하는 것이므로 계정 주인이 안내를 읽고 정하게 한다.

## 결과

- 터미널 카드는 채팅 UI 카드보다 보이는 것이 적다: 도구 단계·토큰·변경 요약이 없고, 결과는 완료 문서와 `o changes`(작업 폴더 비교)로 본다.
- 앱 안 터미널이라도 Google이 제3자 소프트웨어로 볼 가능성을 O가 없앨 수는 없다. 설정 화면의 agy 항목에 이 한계를 적는다.
- 구현 순서: ① 터미널 보기와 Devin 터미널 카드(시험) → ② agy 총괄 터미널과 작업 에이전트 스위치 → ③ ACP 채팅 UI(별도 ADR).
- 채택하면 ADR-0020에 개정 줄을 더하고, AGENTS.md의 Antigravity 문단과 `docs/ARCHITECTURE.md`를 함께 고친다.

## 구현 메모

- 2026-11-11: 카드 전달 기록은 과제 폴더의 `delivery.json`에 작업 ID, 일회용 값, UTC 전달 시각과 `task.md`의 SHA-256으로 남긴다. 완료 문서와 총괄 계획 파일은 각각 최대 1 MiB이며 일반 파일 한 개의 링크만 받는다.
- 2026-11-11: 총괄 터미널은 별도 `o _term` 호스트가 유지하고 카드 터미널은 카드 러너가 호스트한다. 카드의 자원 예약은 터미널 프로세스 트리의 종료가 확인된 뒤에 해제한다.
- 2026-11-11: 붙여넣기 승인 대기는 최대 30분이고, 승인 후 CLI 입력 준비 대기는 최대 60초, 실제 붙여넣기 후 카드 실행은 최대 60분이다. 계획 파일 처리 이름은 터미널 세션 폴더에 기록한다.
- 2026-11-11: 총괄 터미널 계획은 기본 project_O 프로필과 컨텍스트 설정으로 정책 검사한다. 실행은 일반 계획 카드에서 사람이 결정한다.
- 2026-11-11: 계획 파일은 크기와 수정 시각이 1.5초 이상 그대로인 비어 있지 않은 일반 파일만 처리한다. 유효한 JSON이 완성 기준이며 잘린 JSON은 오류로 기록하고 내용이 수정되면 해시가 바뀌어 다시 받는다. 1 MiB 초과 파일은 읽지 않고 이름·크기·수정 시각으로 중복 오류를 막는다.
- 2026-11-11: 완료 문서는 첫 줄의 `O-DONE`과 마지막 비어 있지 않은 줄의 `O-END`가 같은 일회용 값일 때 받으며 두 줄 사이가 결과다. 500ms 안정성 검사도 거치고 임시 파일을 완성한 뒤 `done.md`로 이름을 바꾸라고 안내한다.
- 2026-11-11: 터미널 입력은 세션별 잠금 아래 증가하는 순서로 전달하며 한 번에 최대 64 KiB를 받는다. 총괄 터미널 자식에도 `O_AGENT_ROLE=lead`를 표시해 사람 전용 명령을 제한한다.
- 2026-11-11: 터미널 카드의 `model`·`effort`는 CLI 현재 설정을 사용하므로 계획 검사에서 위반 대신 안내한다. 시작 전에 프로세스 생성이 실패하면 예약을 해제하고, 생성됐을 수 있으면 트리 종료가 확인될 때만 해제한다.
- 2026-11-11: 로컬 프로세스는 Origin 헤더를 위조할 수 있으므로 개발 서버는 사람 전용 설정 변경과 터미널 열기·입력·붙여넣기 승인·닫기 경로를 제공하지 않는다. 앱과 `o` 명령에서만 실행한다.
