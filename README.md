# project_O

여러 AI 코딩 에이전트와 구독 계정을 한 데스크톱 앱에서 실행하고(작업대), 돌고 있는 세션을 한 화면에서 관측하는(관제탑) 개인용 로컬 앱. 비유하면 Orca의 채팅 UI 버전.

> 상태: O CLI(작업카드 분배)·정책 자동 차단은 Rust(`o.exe`)로 동작. 화면은 Orca 디자인 시스템을 따른 React 앱으로, 개발 서버에서 이 PC의 실제 실행 기록을 보여 준다. Tauri 셸은 다음 단계([ADR-0001](docs/decisions/0001-stack-tauri-rust-core.md)).

![작업 환경](docs/screenshots/01-overview.png)

화면 캡처와 아키텍처 그림은 [docs/screenshots/](docs/screenshots/README.md).

## 구성

| 폴더 | 내용 |
|---|---|
| `crates/o-guard` | 프로바이더 정책 규칙, 에이전트 훅(`o hook`), 저장소 검사(`o scan`) |
| `crates/o-core` | 계획·이벤트·정책, 공식 CLI 실행과 스트림 해석, 실행 기록, 병렬 실행기, 사용량 조회 |
| `crates/o-cli` | `o` 명령 하나(`o.exe`) |
| `app/` | 화면 (Vite + React 19 + Tailwind 4). 기록 접기·다음 할 일 로직은 `src/core/*.ts` |
| `skills/o/` | 총괄 에이전트가 쓰는 O 스킬 |

- 작업 규칙·브랜치 흐름·정책 차단: [AGENTS.md](AGENTS.md)
- 설계 초안: [docs/DESIGN.md](docs/DESIGN.md)
- 결정 기록: [docs/decisions/](docs/decisions/)

## 시작하기

Rust(stable)와 Node.js 22.18 이상.

```sh
cargo install --path crates/o-cli   # o.exe 설치 (어디서나 `o`)
npm install                         # git 훅(.githooks → o scan) 연결
cargo test --workspace              # 정책·훅·실행기·스트림 해석·팀 대화 테스트
npm test                            # 화면 로직 테스트
o scan --all                        # 전체 파일 정책 검사
```

git 훅과 에이전트 훅은 PATH의 `o`를 부른다. `o`를 고친 뒤에는 `cargo install --path crates/o-cli`로 다시 설치한다.

## 화면 (app/)

```sh
cd app
npm install
npm run dev        # http://localhost:1430 — 이 PC의 O 실행 기록(없으면 기록 재생 데모)
npm run check      # TypeScript
```

`?source=demo`로 기록 재생, `?source=live`로 실시간을 고른다. 개발 서버의 `/api`는 `o runs --json`·`o usage`를 불러 쓰며 이 PC 주소·같은 출처 요청만 받는다.

Orca(MIT)의 디자인 시스템을 따른다: 토큰은 `app/src/styles/orca-theme.css`(원본 복사, 고치지 말고 다시 추출), Geist 글꼴, Tailwind 4, lucide 아이콘. 출처와 라이선스는 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## O CLI

총괄 에이전트(예: Claude Code 세션)가 요청을 작업카드로 나눠 넘기면, O가 정책을 검사한 뒤 각 공식 CLI를 백그라운드에서 병렬로 실행한다. 어느 에이전트·모델로 할지는 총괄이 정한다.

```
사용자 ─▶ 총괄 Claude ─(O 스킬)─▶ o dispatch plan.json
                                   │ 정책 검사 (위반 시 전체 차단, 종료 코드 2)
                                   ├─▶ codex exec --json
                                   ├─▶ agy -p --output-format stream-json
                                   └─▶ claude -p --output-format stream-json
                                         ▼
                      %LOCALAPPDATA%/project_O/runs/<runId>/events.jsonl ─▶ 채팅·사무실·대시보드
```

실행마다 `events.jsonl`(추가만 하는 원본 기록)을 남기고, 화면과 `o status`는 `%LOCALAPPDATA%/project_O/o.db`(SQLite 색인)를 거쳐 읽는다. 색인은 읽을 때마다 새로 붙은 줄만 가져오고, 하루 실행 수·쿨다운·차단 기록도 여기에 둔다. 카드(에이전트 CLI)는 자기 실행 폴더에만 쓴다.

| 명령 | 하는 일 |
|---|---|
| `o check plan.json` | 정책 검사만 |
| `o dispatch plan.json` | 검사 후 백그라운드 실행, runId 출력 |
| `o status [runId]` | 카드별 상태 |
| `o events <runId> [--follow]` | 정규화 이벤트(JSONL) |
| `o stop <runId>` | 중지 |
| `o msg send / read / post` | 팀 대화: 카드끼리, 카드와 총괄(`lead`)이 메시지를 주고받음 |
| `o usage [--json] [--cached]` | Codex·Antigravity 구독 한도를 각 CLI로 조회 |
| `o runs --json` | 모든 실행 기록(화면용) |
| `o hook` / `o scan --all\|--staged\|--pre-push` | 에이전트 훅 / 정책 검사 |
| `o sync-mirror <폴더>` | test·main 거울 fast-forward (예약 작업용) |

계획 형식과 총괄 에이전트 사용법은 [skills/o/SKILL.md](skills/o/SKILL.md). Claude Code에서 쓰려면 이 폴더를 `~/.claude/skills/o`로 복사하거나 링크한다.
