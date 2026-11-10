# project_O 작업 규칙

사람과 에이전트(Claude Code, Codex 등) 모두 이 규칙을 따른다. Claude Code는 `CLAUDE.md`를 통해 이 파일을 읽는다.

## 팀

| 코드 | 개인 브랜치 | GitHub |
|---|---|---|
| A (대표) | `younchan` | `chan12392` |
| B | `yongwoo` (임시 이름 — 합류 시 본인이 정하면 바꾼다) | |
| 은미 | `eunmi` | `yuneunmi814-cmyk` |

## 브랜치

- 보호 브랜치는 5개다: `test_2`, `test_1_hotfix`, `test_1`, `main_hotfix`, `main`. 직접 커밋·push하지 않고 PR 병합으로만 바뀐다.
- 고정 브랜치는 8개다: `main`, `main_hotfix`, `test_1`, `test_1_hotfix`, `test_2`, `younchan`, `yongwoo`, `eunmi`. 이 외의 최상위 브랜치를 만들지 않는다.
- 각자 자기 개인 브랜치에서 작업한다. 개인 브랜치에서 한 단계 더 갈라진 작업 브랜치(브랜치의 브랜치)를 허용한다.
  - 이름: `<개인 브랜치>-<주제>` (예: `yongwoo-session-resume`). 작업 브랜치에서 다시 가지를 치지 않는다.
  - `/`로 나누지 않는 이유: git은 `younchan` 브랜치가 있으면 `younchan/<주제>`를 만들 수 없다.
  - 사람마다 동시에 살아 있는 작업 브랜치는 최대 3개. 병합하면 바로 삭제한다.
- 상대의 개인 브랜치·작업 브랜치에 커밋하지 않는다.
- 개인 브랜치 목록은 `crates/o-guard/src/hook.rs`의 `PERSONAL_BRANCHES`와 `.github/workflows/branch-flow.yml`에 함께 적혀 있다. 대표 GitHub 계정은 `crates/o-guard/src/rules.rs`의 `OWNER`와 `.github/workflows/merge-guard.yml`에 함께 적혀 있다. 바꿀 때 두 곳을 같이 고친다.

## 병합 흐름

```
개인·작업 브랜치 ──PR──▶ test_2 ──PR──▶ test_1 ──PR──▶ main
핫픽스: 개인·작업 브랜치 ──PR──▶ test_1_hotfix ──PR──▶ test_1
핫픽스(정식): 작업 브랜치 ──PR──▶ main_hotfix ──PR──▶ main
```

| PR | 올릴 수 있는 브랜치 | 병합하는 사람 |
|---|---|---|
| → `test_2` | 개인 브랜치(`younchan`, `yongwoo`, `eunmi`)와 그 작업 브랜치(`<개인>-<주제>`) | 누구나 (자유 통합) |
| → `test_1_hotfix` | 개인·작업 브랜치(`test_1`에서 갈라 만든 급한 수정), `test_1` 동기화 | 대표(A)와 대표의 에이전트만 |
| → `test_1` | `test_2`(`test_2`에서 문제없이 돌아간 것), `test_1_hotfix` | 대표, 또는 다른 팀원 1명 승인 후 누구나 (ADR-0031) |
| → `main_hotfix` | 개인·작업 브랜치(`main`에서 갈라 만든 정식 릴리스의 급한 수정), `main` 동기화 | 대표(A)와 대표의 에이전트만 |
| → `main` | `test_1`, `main_hotfix` | 대표(A)와 대표의 에이전트만 |
| → 개인 브랜치 | 자기 `<개인>-<주제>`, 또는 `main`·`test_1` 동기화 | 본인 |

1. 각자 개인 브랜치나 작업 브랜치에서 `test_2`로 PR을 올리고, 누구든 병합한다.
2. `test_2`에서 함께 돌려 보며 오류·보완점을 찾는다. 수정은 `test_2`에 직접 하지 않고 개인·작업 브랜치에서 고쳐 다시 PR로 올린다.
3. `test_2`에서 확인된 것을 `test_2` → `test_1` PR로 올린다. 작성자가 아닌 팀원 1명이 승인하면 누구나 병합할 수 있고, 대표는 승인 없이 병합할 수 있다(GitHub 규칙 집합이 강제, ADR-0031).
4. `test_1` → `main` PR은 대표가 병합한다.
5. 병합 후 각자 `main`(또는 `test_1`)을 개인 브랜치에 병합해 맞춘다.
6. 배포된 빌드의 급한 수정은 `test_1`에서 가른 작업 브랜치 → `test_1_hotfix` → `test_1`로 올린다(ADR-0021). `test_1`에 들어간 뒤 개인 브랜치에 `test_1`을 병합해 `test_2`로 올려 되맞춘다.
7. 정식 릴리스(`main`)의 급한 수정은 `main`에서 가른 작업 브랜치 → `main_hotfix` → `main`으로 올린다(ADR-0031). 병합은 대표만 한다. `main`에 들어간 뒤 개인 브랜치에 `main`을 병합해 `test_2`로 올려 되맞추고, `test_1`에도 넣는다(`main`을 병합한 개인 브랜치 → `test_1_hotfix` → `test_1`, 또는 다음 `test_2` → `test_1` 릴리스). `main_hotfix`를 `main`에 맞출 때는 `main` → `main_hotfix` 동기화 PR을 쓴다.

- 릴리스: `test_1` push마다 프리릴리스(`release.yml`, 전체 검사와 설치 연기 시험을 통과해야 게시), `main`은 정식 릴리스로 대표가 게시한다. `main` 릴리스 전에 대표가 자기 PC에서 `pwsh scripts/release-gate.ps1`(실제 Claude·Codex 카드, 사람 확인 포함)을 돌린다(ADR-0038).

- 대표의 에이전트는 대표의 `gh` 로그인(`chan12392`)으로 돌기 때문에 대표와 같이 취급한다.
- PR 교차 검토(ADR-0006, ADR-0012): 필요할 때 사람이나 `o review <PR> --post`로 검토한다. 검토 자체는 병합의 필수 조건이 아니다. `o review`는 검토 댓글만 남기고 승인이나 병합은 하지 않는다. 병합은 위 표의 규칙을 따른다.
- 병합 방식은 merge commit만 쓴다(squash·rebase 병합은 저장소에서 꺼 두었다). 브랜치 간 이력이 어긋나지 않게 하기 위해서다.
- 저장소는 GitHub Pro이고 규칙 집합(ruleset)이 보호 브랜치 5개에 모두 걸려 있다(ADR-0031 개정 1). 대표(저장소 관리자)는 모두 우회할 수 있다.
  - 5개 공통: PR로만 바뀌고(merge commit만), CI `policy`·`check`가 통과해야 병합되며, 강제 push·삭제를 막는다. Windows 검사 `windows`(ADR-0038)는 대표가 규칙 집합의 필수 검사에 더해야 병합을 막는다.
  - `test_1`: 작성자가 아닌 사람 1명의 승인이 있어야 병합된다.
  - `test_1_hotfix`·`main_hotfix`·`main`: 업데이트 제한으로 대표만 병합할 수 있다. 팀원은 PR만 연다.
- 팀원이 아닌 사람은 쓰기 권한이 없어 포크에서 PR만 열 수 있고, 그 PR은 `branch-flow.yml`이 실패시킨다(CONTRIBUTING.md).
- 아래 지점에서도 막거나 알린다. 검사가 빨간색이면 병합하지 않는다.
  - 에이전트 훅(`o hook`): 보호 브랜치 커밋·push, 대표가 아닌 `gh` 로그인으로 `test_1_hotfix`·`main_hotfix`·`main`에 병합(`gh pr merge`, `gh api` 병합 호출)을 막는다. `test_1` 병합은 GitHub가 승인을 확인하므로 막지 않는다. PR의 base나 로그인 계정을 확인하지 못하면 막는다(base를 모르면 대표도 막는다).
  - git 훅(`o scan`): 보호 브랜치 커밋, 보호 브랜치로의 push를 막는다.
  - CI: `branch-flow.yml`은 흐름에 맞지 않는 PR을 실패시키고, `merge-guard.yml`은 대표가 아닌 사람이 `test_1_hotfix`·`main_hotfix`·`main`에 병합·push하면 실패로 알린다(사후 경보 — 이미 병합된 것을 막지는 못한다. 대표가 되돌린다).
  - 웹 화면의 병합 버튼도 `test_1_hotfix`·`main_hotfix`·`main`에서는 GitHub 규칙 집합이 대표 외에는 막는다.

## 작업 폴더 (Orca 프로젝트 `project_O`)

| 폴더 | 브랜치 | 용도 |
|---|---|---|
| `orca/projects/project_O` | 자기 개인 브랜치 또는 작업 브랜치 | 작업 |
| `orca/workspaces/project_O/test_2` | `test_2` | **읽기 전용 거울.** 병합된 작업을 함께 돌려 보는 곳 |

- 보호 브랜치(`test_1`·`test_1_hotfix`·`test_2`·`main_hotfix`·`main`)가 체크아웃된 폴더는 에이전트가 바꿀 수 없다. 파일 수정, 허용 목록 밖의 셸 명령(조회·테스트·`git pull --ff-only`만 허용), 커밋, push, O 작업 전달이 모두 막힌다. 다른 폴더의 세션이 경로로 거울 파일을 고치려 해도 막는다.
- 거울은 `test_2`를 따른다. 예약 작업이 5분마다 `origin/test_2`로 fast-forward한다(`o sync-mirror`, 설치는 `scripts/install-mirror-sync.ps1`, 로그 `%LOCALAPPDATA%\project_O\mirror-sync.log`). 동기화는 보호 브랜치(`test_1`·`test_1_hotfix`·`test_2`·`main_hotfix`·`main`) 체크아웃만 받는다. 로컬 변경이 있으면 지우지 않고 동기화를 멈춘 뒤 로그에 남긴다.
- 예약 작업은 저장소 밖에 설치된 `o.exe`를 실행한다(거울이 뒤처져도 동기화가 돌도록). 동기화의 git 호출은 저장소 git 훅을 쓰지 않는다.
- 각자 PC에서 한 번 설정한다: `cargo install --path crates/o-cli` → `git worktree add <거울 폴더> test_2` → `pwsh scripts/install-mirror-sync.ps1 -Mirror <거울 폴더>`. 예전 `test` 거울이 있으면 예약 작업 `project_O mirror sync (test)`를 지우고 새로 설정한다.

## 설계 변경

- 공용 경계에 영향을 주는 결정은 `docs/decisions/`에 ADR로 남긴다. 양쪽의 별도 승인 대기는 요구하지 않는다(ADR-0012).
  - 공용 경계: 기술 스택, 공용 데이터 모델(세션·계정·프로바이더), 프로바이더 연결 방식, 라우팅 단위, 화면 최상위 구조.
- 자기 영역 안의 구현 결정은 ADR 없이 진행한다.
- 현재 설계 초안과 미결정 목록은 `docs/DESIGN.md`에 있다.
- 모듈 지도와 새 프로바이더·화면·명령을 더하는 체크리스트는 `docs/ARCHITECTURE.md`에 있다 (ADR-0007).

## 보안

- 구독 계정 인증 정보·토큰·`.env`는 커밋하지 않는다. 계정 데이터는 각자 PC에만 둔다.
- 테스트·데모 데이터에 실제 계정명·이메일을 넣지 않는다.

## 깨끗한 에이전트 (테스트용)

개인 하네스(사용자 훅·전역 지시·스킬·플러그인·MCP·메모리)가 없는 공식 CLI로 테스트할 때 쓴다. 평소 설정(`~/.claude`, `~/.codex`, `~/.gemini`, `~/.agents`)은 건드리지 않는다.

- 프로필: `%LOCALAPPDATA%/project_O-agents/<claude|codex|agy|opencode>/home` (`o agents path`). O 상태 폴더(`%LOCALAPPDATA%/project_O`, 보호 대상) 밖에 둔다. 로그인은 CLI마다 한 번 `o agents login <claude|codex|opencode>` (agy는 시스템 로그인을 그대로 씀). OpenCode는 카드만 지원하고 Claude 구독 로그인을 사용하지 않는다(ADR-0015). YOLO가 아닌 OpenCode 카드는 작업 폴더나 그 상위에 `opencode.json(c)`·`.opencode/`가 있으면 시작하지 않는다(프로젝트 플러그인을 끌 공식 옵션이 없음). O는 Codex 프로필에 프로젝트 신뢰를 기록하지 않는다. 다만 Git 저장소에서 Codex 카드가 한 번 돌면 Codex가 스스로 그 저장소를 신뢰로 기록하고, 그 뒤의 Codex 카드는 저장소의 `.codex/` 설정·MCP·훅을 싣는다(Git 저장소가 아닌 폴더는 기록되지 않음, ADR-0017 개정 2).
- Devin은 설정 폴더를 옮길 수 없어 project_O 프로필을 켠 계획에서도 Devin 카드만 평소 프로필로 실행하며 개인 훅·설정이 적용된다고 알린다. 총괄은 아직 지원하지 않는다(ADR-0016, ADR-0042 §5).
- Antigravity 터미널 카드는 project_O 프로필 선택과 관계없이 시스템 로그인과 평소 환경을 쓴다. 작업 에이전트 설정이 켜져 있어야 하며, 터미널 안의 권한 확인에는 사람이 답한다(ADR-0042).
- 확인: `o agents doctor` — 로그인 상태와 프로필 오염 검사. 수동 테스트: `o agents shell <cli>`. 카드 실행: `o dispatch plan.json`(기본 `--profile project-o`, `--clean`과 같음). 평소 CLI 설정으로는 `--profile own`. 앱의 "project_O 프로필 사용"이 같은 선택이다(ADR-0013).
- Windows에서 project_O 프로필의 Codex는 샌드박스 점검을 통과해야 카드를 맡는다. 설정 줄(`config.toml`의 `[windows] sandbox`)이나 Codex의 준비 상태만으로는 부족하다(첫 명령에서 멈추는 Codex 버그 openai/codex#50725). 점검은 설정 → 에이전트의 "Codex 샌드박스 점검"이나 `o agents setup codex`로 사람이 돌린다: 창 없이 `codex app-server`로 공식 샌드박스 설정을 마치고, 카드와 같은 `codex exec`로 명령 하나를 실제로 실행해 본다(모델 호출 1번, 최대 약 7분). 결과는 `<에이전트 루트>/codex/sandbox-check.json`에 남고, 통과 기록의 Codex 버전·`config.toml` 해시가 지금과 같을 때만 준비됨이다. 그 전에는 YOLO가 아닌 Codex 카드를 `profile-not-ready`로 거부하고(계획 검사·카드 시작 직전), 총괄에게 매 턴 알려 다른 에이전트로 계획하게 한다(ADR-0040 개정 1). project_O 프로필의 Codex는 사람의 `TEMP` 대신 프로필 옆의 `<에이전트 루트>/codex/tmp`를 임시 폴더로 쓴다(항목이 많은 `TEMP`에 샌드박스 권한을 적용하다 첫 명령 전에 멈추던 원인, 개정 2).
- 테스트 프로젝트는 홈 폴더 밖에 둔다(Claude는 시작 폴더 위의 지시 파일을 읽는다. 홈 폴더의 것만 프로필이 뺀다).
- Orca 터미널은 자체 훅을 더하므로 `o agents shell`이나 일반 터미널에서 쓴다.
- O 코드는 로그인 파일을 만들거나 읽지 않는다. 로그인 상태는 각 CLI의 상태 명령으로만 본다.

## 데스크톱 셸 (`app/src-tauri`)

- Tauri 2 셸. 크레이트 `project-o`, 실행 파일 `project_O.exe`, 창 제목 "Project O". 빌드: `cd app && npm run tauri build -- --no-bundle` → `target/release/project_O.exe`. 개발: `npm run tauri dev`(Vite 1430 포트, `tauri.conf.json`의 `devUrl`·`devCsp`와 같이 고친다).
- 미리보기: `cd app && npm run preview:app`. 설치한 앱을 그대로 둔 채 "Project O 미리보기" 창을 따로 띄운다(`tauri.preview.conf.json`이 식별자를 바꿔 단일 실행 잠금이 겹치지 않는다. 상태 폴더는 같은 것을 쓴다). 화면 코드는 저장하면 바로 바뀌고, `app/src-tauri`·`o-core`를 고치면 셸을 다시 빌드해 창을 다시 연다. `o.exe`(o-cli)는 미리보기를 다시 시작할 때만 새로 빌드된다. 수정할 때마다 설치 파일을 만들지 않고 여기서 확인한다.
- 총괄 터미널 세션은 별도 `o _term` 호스트가, 카드 터미널은 카드 러너가 유지하며 `o term open|list|read|write|resize|paste|close`로 다룬다. 총괄 대화에 연결된 터미널은 `project_O-lead/<threadId>/plans/`의 계획을 검사해 실행 대기 카드로 보인다. 터미널 카드의 과제·전달 기록·완료 문서는 상태와 프로젝트 밖 `%LOCALAPPDATA%/project_O-term/<runId>/<cardId>/`에 남는다(ADR-0042).
- 총괄 터미널 자식도 총괄 표식을 가져 사람 전용 설정·터미널 명령을 실행하지 못한다. 입력은 세션별 순서로 전달하고 한 번에 최대 64 KiB를 받는다. 계획 파일은 1.5초, 완료 문서는 500ms 안정돼야 받으며 완료 문서는 임시 파일을 완성한 뒤 이름을 바꿔 전달하고 첫 줄 `O-DONE`과 마지막 줄 `O-END`에 일회용 값을 적는다. 계획은 이름과 내용 해시로 중복을 판별한다.
- 화면은 프로세스를 직접 띄우지 않는다. 셸의 정해진 Tauri 명령(`app/src-tauri/src/commands/`)만 부르고, 그 명령은 `o-core`를 쓴다. 명령을 더하면 개발 서버 `/api`(`app/dev/liveRuns.ts`)와 `o` 명령에도 같은 동작을 두되, 사람 전용 agy 설정 변경과 터미널 열기·입력·붙여넣기·닫기는 개발 서버에 두지 않는다.
- 자원 예약 점검에서 실행기와 작업 프로세스만 종료된 `unknown` 예약을 복구하려면 사람이 관련 자손 프로세스의 종료를 직접 확인해야 한다. 앱의 확인 단계 또는 `o resources recover <id> --confirm-exited`를 쓰며, 코어는 두 프로세스의 신원을 복구 직전에 다시 검사한다(ADR-0039 개정 3).
- 에이전트 설치는 공식 npm 패키지만 앱에서 실행하고, 나머지는 공식 안내를 연다. 로그인은 새 콘솔 창의 공식 명령으로 한다.
- 설치 파일(NSIS, 사용자별, `o.exe` 사이드카 포함): `pwsh scripts/build-installer.ps1`, `test_1` push마다 `release.yml`이 전체 검사·설치 연기 시험 뒤 프리릴리스로 올린다. 코드 서명은 `PROJECT_O_SIGN_*` 설정이 있을 때만 하고, 없으면 미서명 안내를 내고 계속한다(ADR-0038). 설치 폴더·상태 폴더 구분과 안내는 [docs/INSTALL.md](docs/INSTALL.md).
- Linux CI(job `policy`)는 WebView 라이브러리가 없어 `cargo test/clippy`에서 `project-o`를 뺀다(`.github/workflows/guard.yml`). Windows CI(job `windows`)는 화면을 빌드한 뒤 `project-o`까지 포함해 `cargo test --workspace`와 `cargo clippy --workspace --all-targets -- -D warnings`를 돌린다(ADR-0038). Windows 전용 코드의 import·`mut`는 `#[cfg(windows)]`로 묶어 Linux clippy가 통과하게 한다.
- 테스트·검사는 개발 설정으로 돌린다: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo run -p o-cli -- scan --all`(`--release` 없이). `--release`(lto·codegen-units 1)는 설치 파일과 배포용 `o.exe`에만 쓴다. 테스트를 release로 빌드하면 파일 하나 고친 뒤 다시 빌드하는 데 약 20배 오래 걸린다(162초 대 7초).
- CI(`guard.yml`)는 PR마다 돈다(같은 PR의 새 push는 이전 실행을 취소). push로는 다시 돌지 않고, `release.yml`이 게시 전에 같은 커밋에서 한 번 더 부른다. 모든 PR이 건너뛰는 단계 없이 전체 검사(o scan, Rust 테스트·clippy, TS 테스트·타입 검사, 라이선스 검사, Windows job)를 돈다. 병합 결과가 head와 같다고 건너뛰던 규칙은 없앴다(ADR-0038). 포크 PR도 전체 검사를 돌고 `branch-flow.yml`이 거부한다. PR 없이 push만 한 개인 브랜치는 CI가 돌지 않으니 로컬에서 검사한다.

## 프로바이더 정책 자동 차단

Antigravity 총괄은 사람이 직접 쓰는 터미널이다. 작업 에이전트는 기본 꺼짐이며 사람이 안내와 약관을 읽고 `o agy --workers on --ack`로 켜면 터미널 카드로 실행된다. `o agy --confirm-paste on|off`의 기본값은 확인함이고, 작업·완료 파일로 주고받으며 후속 지시는 받지 않는다. 창 없는 총괄·예약 실행과 자동 계정·모델·사용량 조회는 계속 보류한다(ADR-0020 개정 1, ADR-0042 §3). 아래의 Antigravity 헤드리스 실행 설명은 역사적 기록이다.

터미널 카드는 `model`·`effort` 대신 CLI의 현재 설정을 쓰며 계획 검사에서 이를 안내한다.

사람이 매번 약관을 확인하지 않도록 아래 지점에서 자동으로 막는다. 규칙 원문은 `crates/o-guard/src/rules.rs`, 근거는 `docs/DESIGN.md` §8.

| 지점 | 무엇을 막나 |
|---|---|
| 에이전트 훅 (`.claude/settings.json`, `.codex/hooks.json` → `o hook`, `crates/o-guard`) | 인증 파일·토큰 접근(Read·Grep 포함), 프로바이더 서버 직접 호출, 권한 확인 끄는 옵션이 들어간 명령·코드 수정 / 보호 브랜치(test_1·test_1_hotfix·test_2·main_hotfix·main) 커밋·push / 대표 외 test_1_hotfix·main_hotfix·main 병합(`gh pr merge`·`gh api`) / 규칙 밖 브랜치 이름. 훅이나 gh 확인이 실패하면 막는다. `o`가 PATH에 없거나 훅이 오류로 끝나도 막는다: Claude는 `"onFailure": "block"`(Claude Code 2.1.295 이상, 시간 초과도 막음), Codex는 `o hook \|\| exit 2`(Codex는 시간 초과를 통과시킨다) |
| git 훅 (`.githooks/` → `o scan`, `npm install` 시 자동 설정) | 같은 코드 규칙으로 커밋 검사, 보호 브랜치 커밋·push |
| CI (`.github/workflows/guard.yml`, `branch-flow.yml`, `merge-guard.yml`) | 전체 파일 정책 검사 + 테스트 + 타입 검사(Linux·Windows), PR 방향 검사, 대표 외 test_1_hotfix·main_hotfix·main 병합 경보 (읽기 전용 토큰) |
| O CLI (`crates/o-core/src/policy.rs`) | 카드에 실행 파일·인자·환경변수·계정 지정, 허용 외 프로바이더, 한도에 걸린 서비스 재사용(다른 계정 포함), 동시 실행·하루 실행 상한 초과(카드 시작 직전에도 확인), O 상태 폴더·홈 폴더를 품는 작업 폴더, 정책 위반 내용의 팀 메시지, 카드의 총괄·다른 실행 사칭 (ADR-0002), 오늘 사용 예산을 다 쓴 서비스(`daily-budget`, 하루 사용 예산이 켜져 있을 때만 — 기본 켜짐, `o budget --on\|--off`. 카드 시작 직전에도 확인 — ADR-0004, ADR-0013), 현재 설계 대화에서 분배하지 않은 실행으로의 총괄 후속 지시(ADR-0022) |

- 에이전트 훅은 사람 전용 `o resources recover <id> --confirm-exited`도 차단한다(ADR-0039 개정 3).

- YOLO가 꺼진 기본 모드에서 총괄 CLI는 읽기·검색만 바로 하고, 파일 수정·명령 실행·웹 요청은 사람이 입력창 위 승인 카드에서 하나씩 허용해야 실행된다(ADR-0008 개정 3). "항상 허용"은 없고, 요청마다 10분 안에 답이 없으면 거부, "중지"는 기다리는 요청을 거부한다. 묻기 전에 O 가드 규칙(로그인 정보·보호 브랜치·권한 우회 등)과 O 상태 폴더에 걸리는 요청은 바로 거부한다. 결정은 앱 버튼이나 사람이 친 `o lead decide`뿐이며 에이전트 환경에서는 거절된다(채팅의 "승인"은 결정이 아니다). Claude는 `--tools`에 쓰기·셸·웹 도구를 더하되 `--allowedTools Read,Grep,Glob`, O의 승인 MCP 서버(`o _lead-approve`) 하나만 싣고 `--permission-prompt-tool mcp__o_lead__decide`, Codex는 `codex app-server`에 읽기 전용 샌드박스와 승인 `on-request`(승인 검토자 `user`, ADR-0023 개정 — Windows에서는 셸 읽기 명령도 승인을 묻는다). 예약 실행·카드 질문 턴과 승인 경로가 없는 경우(O CLI를 못 찾음, app-server가 없는 Codex는 `codex exec -s read-only --disable hooks`)는 이전처럼 읽기 전용이다. Antigravity는 보류 전 기준으로 `--mode plan --sandbox`에 파일 쓰기·셸·MCP·URL 동작을 모두 거부하는 세션 설정(plan 모드만으로는 막히지 않아 거부 규칙이 필수, ADR-0008 개정 1). 계획 승인 전에 프로젝트 훅이 돌지 않게 Claude는 `--setting-sources user --settings {"disableAllHooks":true,…}`(승인 경로에서는 쓰기·셸·웹 도구를 `permissions.ask`로 둠), Codex는 `--disable hooks`(app-server·exec 모두), agy는 프로젝트에 `.agents/hooks.json`이 있으면 총괄을 맡지 않는다(개정 2). 총괄 모델은 고르지 않으면 CLI 기본이고, 고르면 카드 `model`과 같은 형식 검사를 거친다. 총괄의 계획은 `o dispatch`와 같은 정책을 두 번(저장할 때·실행할 때) 거치고, 사람이 "실행"을 눌러야 한 번만 전달된다. 예외는 총괄의 후속 지시(`o-followup`, ADR-0022)다: 현재 설계 대화에서 사람이 승인해 분배한 실행(과 그 후속 실행)의 카드에만, 그 대화가 아직 현재 대화일 때 "실행" 없이 접수된다. 분배 전이거나 "새 설계"로 대화가 바뀌었으면 보내지 않으며, 새 작업은 새 설계에서 계획을 다시 승인해야 한다. 첨부는 O 상태 폴더 밖(`%LOCALAPPDATA%/project_O-lead`)에 새 파일로만 저장하고 링크·로그인 정보 위치는 받지 않는다. CLI 인증 환경 전달과 API 과금 경고는 ADR-0035를 따른다.
- YOLO가 켜지면 총괄과 카드가 승인 없이 파일과 명령을 다룬다(ADR-0014). 하루 예산이 켜져 있고 제한에 맞는 계획은 자동 실행한다. O 표식·CLI 거부 규칙·가드로 에이전트의 직접 전달을 제한하지만, 같은 Windows 사용자 권한에서 표식 제거 또는 별도 프로세스를 이용한 우회는 막을 수 없다. Claude YOLO는 O 가드 훅을 주입하며 Codex·Antigravity에는 동일한 훅 적용이 확인되지 않았다.
- 기본 모드의 카드 CLI에 열어 주는 권한은 팀 대화 명령(`o msg`)과 읽기 전용 컨텍스트 검색(`o context`, ADR-0005)뿐이다: Claude는 `--allowedTools "Bash(o msg *)" "Bash(o context *)"`, Codex는 실행 폴더 `--add-dir`(셸 명령은 샌드박스 안에서 그대로 실행), Antigravity는 정확히 `o msg read`·`o msg post` 두 명령(그 CLI의 허용 규칙은 전체 일치만 지원해 `o context`는 열지 않고, 프롬프트에 주입된 컨텍스트만 쓴다). 카드 안의 `o context`는 o.db를 읽기만 하고 `o context add`는 거부한다. 그 밖의 권한 확인은 그대로 둔다.
- API 과금 모드(ADR-0035 개정 1): `o billing --warn|--subscription`, 앱 설정 → 에이전트 → API 과금. 기본 `warn`은 인증 환경변수를 넘기고 이름만 경고한다. `subscription`(구독만)은 그 CLI가 쓰는 인증 변수가 넘길 환경에 있으면 카드·후속 지시·검토·총괄 턴·그래프 Deep을 시작하지 않고 변수 이름을 알린다. 변수를 지우거나 값을 읽지 않으며, 상태·사용량·모델 조회는 두 모드 모두 돈다. 카드·총괄 안에서는 모드를 바꿀 수 없다.

- 차단되면 우회하지 않는다. 사유를 그대로 상대에게 공유하고, 규칙이 틀렸다면 ADR로 바꾼다.
- `crates/o-guard/src/rules.rs`의 규칙·허용 경로, `crates/o-core/src/policy.rs`의 `LIMITS`(화면 표시용 사본 `src/core/limits.ts`)는 공용 경계다(ADR 기록). 설정·환경변수로 상한을 올리는 길은 일부러 두지 않았다.
- Codex는 프로젝트 훅을 처음 쓸 때와 훅 명령이 바뀔 때 신뢰 승인을 요구할 수 있다. 훅이 안 돌아도 git 훅과 CI가 같은 검사를 한다.

## 커밋·PR

- 커밋 메시지: `<type>: <요약>` (`feat`, `fix`, `docs`, `refactor`, `test`, `chore`).
- PR 본문은 `.github/pull_request_template.md`를 채운다. 무엇을 바꿨는지와 어떻게 확인했는지를 적는다.
