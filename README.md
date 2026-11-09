# project_O

Claude Code·Codex 같은 AI 코딩 에이전트의 공식 CLI를 사용자 본인의 로그인으로 한 데스크톱 앱에서 실행하고(작업대), 돌고 있는 세션을 한 화면에서 관측하는(관제탑) 개인용 로컬 앱. 비유하면 Orca의 채팅 UI 버전.

**English summary.** project_O is a personal, local-only desktop app and CLI (`o`) for Windows. It runs official coding-agent CLIs such as Claude Code and Codex under your own logins, lets a lead agent split a request into work cards, runs the cards in parallel within fixed policy limits, and shows every session in one window. It has no server, never reads or stores login tokens, and does not pool or rotate accounts. Documentation is in Korean.

> Anthropic·OpenAI·Google·Cognition 등과 제휴하지 않은 개인 프로젝트이며, Claude·Codex·Gemini·Antigravity·Devin 등 제품명은 각 회사의 상표다.

> 상태: O CLI(작업카드 분배)·정책 자동 차단은 Rust(`o.exe`)로 동작. 화면은 Orca 디자인 시스템을 따른 React 앱으로, 개발 서버나 데스크톱 앱(`project_O.exe`, Tauri 2 셸)에서 이 PC의 실제 실행 기록을 보여 준다([ADR-0001](docs/decisions/0001-stack-tauri-rust-core.md)).

![작업 환경](docs/screenshots/01-overview.png)

*Antigravity 실행 보류([ADR-0020](docs/decisions/0020-antigravity-policy.md)) 전 화면.* 화면 캡처와 아키텍처 그림은 [docs/screenshots/](docs/screenshots/README.md).

## 쓰기 전에

- **본인 구독, 공식 CLI**: O는 각 공급자의 공식 CLI(`claude`, `codex` 등)를 사용자가 직접 로그인한 상태로 실행한다. 구독과 로그인은 사용자 본인의 것이고, 각 공급자의 이용 약관·사용 정책을 지킬 책임도 사용자에게 있다. O는 로그인 파일·토큰을 읽거나 복사하지 않고 공급자 서버를 직접 부르지 않는다. 한 서비스에 여러 계정을 등록해 바꿔 쓰는 기능은 없고, 한도에 걸린 서비스는 다른 계정으로도 다시 쓰지 않도록 막는다. Antigravity는 공급자 정책에 따라 실행하지 않는다([ADR-0020](docs/decisions/0020-antigravity-policy.md)).
- **YOLO 모드**: 기본은 꺼져 있다. 켜면 총괄과 카드에 각 CLI의 권한 확인을 끄는 공식 옵션을 넘겨, 에이전트가 승인 없이 파일을 고치고 명령을 실행한다([ADR-0014](docs/decisions/0014-yolo-mode.md)). 믿을 수 있는 프로젝트에서만 켠다.
- **예약 실행**: 사용자가 만든 예약만 Windows 작업 스케줄러로 앱이 꺼져 있어도 돈다. 총괄은 읽기 전용, 카드는 일반 권한이며 하루 사용 예산과 자동 실행 한도를 통과해야 분배된다([ADR-0019](docs/decisions/0019-navigation-images-schedules.md)).
- **지원 OS**: Windows x64. macOS(Apple Silicon)는 소스에서 직접 빌드하는 실험 단계다. 실기 검증은 한 대의 Mac에서 Codex로만 했고 배포 파일은 없다([ADR-0030](docs/decisions/0030-macos-owned-worker-observation.md), [실기 기록](docs/macos-verification-2026-11-05.md)). 예약 실행은 Windows에서만 된다. Linux 앱은 지원하지 않는다.

## 설치

[Releases](https://github.com/chan12392/project_O/releases)에서 `project_O_<버전>_x64-setup.exe`를 받는다. `test_1-<날짜>-<커밋>` 태그는 `test_1` 브랜치에서 자동으로 만든 프리릴리스(시험판)다. 설치 파일은 코드 서명을 하지 않았으므로 처음 실행할 때 SmartScreen이 "알 수 없는 게시자"를 띄운다. 에이전트 CLI(Claude Code, Codex)는 따로 설치하고 각 CLI의 공식 명령으로 로그인한다(앱의 첫 실행 안내가 돕는다). 설치 폴더·제거·업데이트 알림은 [docs/INSTALL.md](docs/INSTALL.md).

## 보안 모델 (요약)

- **O 가드**: 에이전트 훅(`o hook`, Claude Code·Codex 프로젝트 훅), git 훅(`o scan`), CI가 같은 규칙으로 인증 파일·토큰 접근, 공급자 서버 직접 호출, 권한 확인을 끄는 옵션, 보호 브랜치 커밋·push를 막는다. 규칙 원문은 `crates/o-guard/src/rules.rs`.
- **정책 상한**: `o dispatch`와 앱은 카드로 실행 파일·인자·환경변수·계정을 지정하지 못하게 하고, 동시 실행 6개·하루 카드 60장·한도 쿨다운·하루 사용 예산을 넘는 계획을 분배하지 않는다(`crates/o-core/src/policy.rs`). 상한을 설정이나 환경변수로 올리는 길은 없다.
- **사람의 승인**: 기본 모드에서 총괄은 읽기·검색만 바로 하고, 파일 수정·명령 실행·웹 요청은 사람이 앱에서 하나씩 허용해야 한다. 총괄의 계획도 사람이 "실행"을 눌러야 분배된다(예외: 예약, 이미 승인한 작업의 후속 지시 — [ADR-0022](docs/decisions/0022-agent-session-continuations.md)).
- **막지 못하는 것**: O는 보안 샌드박스가 아니다. 같은 Windows 사용자 권한으로 도는 프로그램이 O의 표식을 지우거나 별도 프로세스를 띄워 우회하는 것은 막을 수 없다. YOLO에서 O 가드 훅은 Claude에만 주입되고, Codex에는 같은 훅 적용이 확인되지 않았다. 각 CLI 자체의 샌드박스·권한 설정을 함께 쓴다.

자세한 표는 [AGENTS.md](AGENTS.md) "프로바이더 정책 자동 차단". 보안 문제는 공개 이슈 대신 [SECURITY.md](SECURITY.md)의 방법으로 알린다.

## 기여

외부 기여 방법, 개발 환경, 검사 명령은 [CONTRIBUTING.md](CONTRIBUTING.md). 팀의 브랜치·병합 규칙은 [AGENTS.md](AGENTS.md).

## 구성

| 폴더 | 내용 |
|---|---|
| `crates/o-guard` | 프로바이더 정책 규칙, 에이전트 훅(`o hook`), 저장소 검사(`o scan`) |
| `crates/o-core` | 계획·이벤트·정책, 공식 CLI 실행과 스트림 해석, 실행 기록, 병렬 실행기, 사용량 조회 |
| `crates/o-cli` | `o` 명령 하나(`o.exe`) |
| `app/` | 화면 (Vite + React 19 + Tailwind 4). 기록 접기·다음 할 일 로직은 `src/core/*.ts` |
| `app/src-tauri` | 데스크톱 셸 (Tauri 2, 크레이트 `project-o` → `project_O.exe`). `o-core`를 직접 쓴다 |
| `skills/o/` | 총괄 에이전트가 쓰는 O 스킬 |

- 팀 작업 규칙·브랜치 흐름·정책 차단(팀원과 에이전트용): [AGENTS.md](AGENTS.md)
- 기여 방법: [CONTRIBUTING.md](CONTRIBUTING.md)
- 모듈 구조·데이터 흐름·새 기능 추가 체크리스트: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
- 설계 초안: [docs/DESIGN.md](docs/DESIGN.md)
- 제품 목표와 현재 상태: [docs/GOALS.md](docs/GOALS.md)
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

### 데스크톱 앱 (project_O.exe)

`app/src-tauri`의 Tauri 2 셸. 같은 화면을 창 하나로 띄우고, 실행 기록·사용량·에이전트 CLI 상태를 `o-core`로 직접 읽는다(개발 서버의 `/api` 대신 Tauri 명령: `runs`, `usage`, `usage_refresh`, `agents_status`, `agent_install`, `agent_login`, `agent_setup`, `open_external`, `about`, `review`).

```sh
cd app
npm install
npm run tauri build -- --no-bundle   # 화면 빌드(dist) 후 target/release/project_O.exe
npm run tauri dev                    # 개발: http://localhost:1430 + 창
```

Windows 설치 파일(NSIS, `o.exe` 포함): [docs/INSTALL.md](docs/INSTALL.md) — `pwsh scripts/build-installer.ps1` 또는 `test_1` 프리릴리스.

- 설치는 공식 npm 패키지(`npm install -g @anthropic-ai/claude-code`, `npm install -g @openai/codex`)가 있고 npm이 PATH에 있을 때만 앱에서 실행한다. npm이 없으면 각 회사의 공식 설치 안내를 보여 준다.
- 로그인은 새 콘솔 창에서 공식 명령(`claude auth login`, `codex login`)을 띄운다. Antigravity는 공급자 정책에 따라 Project O에서 실행하지 않는다.
- project_O 프로필의 Codex는 Windows 샌드박스 점검을 통과해야 카드를 맡는다. 그 전에는 Codex 카드를 시작하지 않고, 화면의 "Codex 샌드박스 점검"(`o agents setup codex`)이 창 없이 Codex의 공식 샌드박스 설정을 마친 뒤 카드처럼 명령 하나를 실제로 실행해 본다(모델 호출 1번). 통과 기록은 Codex 버전이나 프로필 설정이 바뀌면 다시 점검해야 한다(ADR-0040 개정 1).
- 같은 동작을 CLI로: `o agents status [--clean] --json`, `o agents install <id>`, `o agents open-login <id> [--clean]`, `o agents setup codex`, `o about --json`.

`?source=demo`로 기록 재생, `?source=live`로 실시간을 고른다. 개발 서버의 `/api`는 `o runs --json`·`o usage`를 불러 쓰며 이 PC 주소·같은 출처 요청만 받는다.

Orca(MIT)의 디자인 시스템을 따른다: 토큰은 `app/src/styles/orca-theme.css`(원본 복사, 고치지 말고 다시 추출), Geist 글꼴, Tailwind 4, lucide 아이콘. 출처와 라이선스는 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

프로젝트 라이선스는 [Apache-2.0](LICENSE)이다. 제3자 구성 요소는 각자의 라이선스를 따른다.

## O CLI

총괄 에이전트(예: Claude Code 세션)가 요청을 작업카드로 나눠 넘기면, O가 정책을 검사한 뒤 각 공식 CLI를 백그라운드에서 병렬로 실행한다. 어느 에이전트·모델로 할지는 총괄이 정한다.

```
사용자 ─▶ 총괄 Claude ─(O 스킬)─▶ o dispatch plan.json
                                   │ 정책 검사 (위반 시 전체 차단, 종료 코드 2)
                                   ├─▶ codex exec --json
                                   └─▶ claude -p --output-format stream-json
                                         ▼
                      %LOCALAPPDATA%/project_O/runs/<runId>/events.jsonl ─▶ 채팅·사무실·대시보드
```

실행마다 `events.jsonl`(추가만 하는 원본 기록)을 남기고, 화면과 `o status`는 `%LOCALAPPDATA%/project_O/o.db`(SQLite 색인)를 거쳐 읽는다. 색인은 읽을 때마다 새로 붙은 줄만 가져오고, 하루 실행 수·쿨다운·차단 기록도 여기에 둔다. 카드(에이전트 CLI)는 자기 실행 폴더에만 쓴다.

| 명령 | 하는 일 |
|---|---|
| `o check plan.json` | 정책 검사만 |
| `o dispatch [--profile project-o\|own] [--no-context] plan.json` | 검사 후 백그라운드 실행, runId 출력. 기본은 project_O 프로필(깨끗한 에이전트), `own`은 이 PC의 평소 CLI 설정 ([ADR-0013](docs/decisions/0013-profile-and-budget-switches.md)) |
| `o status [runId]` | 카드별 상태 |
| `o events <runId> [--follow]` | 정규화 이벤트(JSONL) |
| `o stop <runId>` | 중지 |
| `o msg send / read / post` | 팀 대화: 카드끼리, 카드와 총괄(`lead`)이 메시지를 주고받음 |
| `o review <PR 번호\|주소> [--provider P] [--model M] [--effort E] [--profile project-o\|own] [--post] [--dry-run]` | 지금 폴더 저장소의 PR을 `gh`로 읽어 카드 하나로 검토(파일 변경 없음). `--post`는 검토문을 PR 댓글로 게시 — 승인·병합은 하지 않음 ([ADR-0006](docs/decisions/0006-pr-cross-review.md)) |
| `o usage [--json] [--cached]` | 지원되는 CLI의 구독 한도를 조회 (Antigravity 조회 보류) |
| `o budget [--json] [--cached] [--percent 12\|13\|14] [--on\|--off]` | 에이전트별 오늘 사용 예산(주간 한도의 12~14%)·속도·effort 조절, 예산 켜기·끄기 (아래) |
| `o runs --json` | 모든 실행 기록(화면용) |
| `o hook` / `o scan --all\|--staged\|--pre-push` | 에이전트 훅 / 정책 검사 |
| `o sync-mirror <폴더>` | 거울(팀은 `test_2`, 다른 보호 브랜치도 가능) fast-forward (예약 작업용) |
| `o agents path\|login\|shell\|doctor` | 깨끗한 에이전트 프로필 (아래) |
| `o agents status\|install\|open-login\|setup` / `o about` | 에이전트 CLI 설치·로그인 상태, 공식 설치·로그인 창, 프로필 설정·점검(Codex 샌드박스) / 버전과 폴더 위치 (데스크톱 앱과 같은 동작) |

### 하루 사용 예산 ([ADR-0004](docs/decisions/0004-daily-usage-budget.md), [ADR-0013](docs/decisions/0013-profile-and-budget-switches.md))

에이전트(프로바이더)마다 하루에 주간(`seven_day`) 한도의 13%(설정 → 에이전트, 또는 `o budget --percent 12|13|14`; `%LOCALAPPDATA%/project_O/settings.json`의 `dailyBudgetPercent`)만 쓴다.

- 오늘 쓴 양 = 지금 주간 사용률 − 오늘 0시의 주간 사용률. 사용량을 읽을 때마다(`o usage`, 앱의 새로 고침, `o budget`, 실행 직전 조회, Claude 실행 중 보고) `o.db`의 `usage_snapshots`에 남긴 기록으로 계산한다. 주간 한도가 오늘 초기화됐으면 초기화 시점부터 센다.
- 카드를 시작하기 직전에 속도를 본다. 남은 예산 비율이 남은 시간 비율(자정, 또는 그 전에 오는 주간 초기화까지)보다 작으면 effort를 한 단계, 절반보다 작으면 두 단계 낮춘다(`low` 아래로는 낮추지 않고, 올리지 않는다). effort가 없는 카드는 `medium`으로 보고 낮춘다. 바꾼 내용은 `card.budget` 이벤트로 남고, plan.json은 그대로다.
- 예산을 다 쓴 에이전트로는 새 카드를 보내지 않는다(정책 `daily-budget`). 주간 기록이 없는 에이전트는 조절하지 않는다.
- 끌 수 있다(설정 → 에이전트 → 하루 사용 예산, `o budget --off`; `settings.json`의 `dailyBudgetEnabled`). 끄면 예산으로 막거나 effort를 낮추지 않고, 숫자와 "오늘" 고리는 "예산 꺼짐"으로 남는다. 구독 한도·쿨다운·`LIMITS`는 그대로다.
- `o _run`은 저장된 지원 공급자의 기록이 10분보다 오래됐으면 먼저 다시 조회한다. `o dispatch`는 조회하지 않고 저장된 기록으로 검사한다.

### 깨끗한 에이전트 (테스트용)

개인 하네스(사용자 훅·전역 AGENTS.md/CLAUDE.md·스킬·플러그인·MCP·메모리·사용자 설정)가 하나도 없는 공식 CLI를 평소 설정을 건드리지 않고 쓴다.

| 명령 | 하는 일 |
|---|---|
| `o agents path` | 프로필 위치: `%LOCALAPPDATA%/project_O-agents/<claude\|codex\|agy>/home` |
| `o agents login <claude\|codex\|opencode>` | 그 프로필로 공식 로그인. CLI마다 한 번 |
| `o agents shell <claude\|codex\|opencode> [인자…]` | 지금 폴더에서 깨끗한 CLI를 대화형으로 실행 |
| `o agents doctor [--json]` | 로그인 상태와 프로필 오염(훅·스킬·MCP·전역 지시) 검사. 깨끗하면 종료 코드 0 |
| `o dispatch plan.json` (`--profile project-o`, `--clean`과 같음) | 이 실행의 카드를 깨끗한 프로필로 실행 (계획 형식은 그대로). 앱의 "project_O 프로필 사용"과 같다 |

깨끗한 실행은 환경변수를 시스템 기본값(PATH, SystemRoot, TEMP, APPDATA, LOCALAPPDATA, ProgramFiles 등)만 남기고 비운 뒤 홈(USERPROFILE/HOME)을 프로필로 바꾸고, Claude는 `CLAUDE_CONFIG_DIR`, Codex는 `CODEX_HOME`을 프로필 안으로 둔다. Claude는 시작 폴더 위의 지시 파일도 읽으므로 프로필 설정(`claudeMdExcludes`)이 홈 폴더의 AGENTS.md·CLAUDE.md를 뺀다. 그 밖의 상위 폴더 지시 파일은 `doctor`가 알려 준다 — 테스트 프로젝트는 홈 폴더 밖에 둔다. Orca 터미널은 자체 훅을 더하므로 `o agents shell`이나 일반 터미널을 쓴다.

계획 형식과 총괄 에이전트 사용법은 [skills/o/SKILL.md](skills/o/SKILL.md). Claude Code에서 쓰려면 이 폴더를 `~/.claude/skills/o`로 복사하거나 링크한다.
