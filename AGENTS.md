# project_O 작업 규칙

사람과 에이전트(Claude Code, Codex 등) 모두 이 규칙을 따른다. Claude Code는 `CLAUDE.md`를 통해 이 파일을 읽는다.

## 팀

| 코드 | 개인 브랜치 | GitHub |
|---|---|---|
| A (대표) | `younchan` | `chan12392` |
| B | `yongwoo` (임시 이름 — 합류 시 본인이 정하면 바꾼다) | |

## 브랜치

- 보호 브랜치는 4개다: `test_2`, `test_1_hotfix`, `test_1`, `main`. 직접 커밋·push하지 않고 PR 병합으로만 바뀐다.
- 고정 브랜치는 6개다: `main`, `test_1`, `test_1_hotfix`, `test_2`, `younchan`, `yongwoo`. 이 외의 최상위 브랜치를 만들지 않는다.
- 각자 자기 개인 브랜치에서 작업한다. 개인 브랜치에서 한 단계 더 갈라진 작업 브랜치(브랜치의 브랜치)를 허용한다.
  - 이름: `<개인 브랜치>-<주제>` (예: `yongwoo-account-vault`). 작업 브랜치에서 다시 가지를 치지 않는다.
  - `/`로 나누지 않는 이유: git은 `younchan` 브랜치가 있으면 `younchan/<주제>`를 만들 수 없다.
  - 사람마다 동시에 살아 있는 작업 브랜치는 최대 3개. 병합하면 바로 삭제한다.
- 상대의 개인 브랜치·작업 브랜치에 커밋하지 않는다.
- 개인 브랜치 목록은 `crates/o-guard/src/hook.rs`의 `PERSONAL_BRANCHES`와 `.github/workflows/branch-flow.yml`에 함께 적혀 있다. 대표 GitHub 계정은 `crates/o-guard/src/rules.rs`의 `OWNER`와 `.github/workflows/merge-guard.yml`에 함께 적혀 있다. 바꿀 때 두 곳을 같이 고친다.

## 병합 흐름

```
개인·작업 브랜치 ──PR──▶ test_2 ──PR──▶ test_1 ──PR──▶ main
핫픽스: 개인·작업 브랜치 ──PR──▶ test_1_hotfix ──PR──▶ test_1
```

| PR | 올릴 수 있는 브랜치 | 병합하는 사람 |
|---|---|---|
| → `test_2` | 개인 브랜치(`younchan`, `yongwoo`)와 그 작업 브랜치(`<개인>-<주제>`) | 누구나 (자유 통합) |
| → `test_1_hotfix` | 개인·작업 브랜치(`test_1`에서 갈라 만든 급한 수정), `test_1` 동기화 | 대표(A)와 대표의 에이전트만 |
| → `test_1` | `test_2`(`test_2`에서 문제없이 돌아간 것), `test_1_hotfix` | 대표(A)와 대표의 에이전트만 |
| → `main` | `test_1`만 | 대표(A)와 대표의 에이전트만 |
| → 개인 브랜치 | 자기 `<개인>-<주제>`, 또는 `main`·`test_1` 동기화 | 본인 |

1. 각자 개인 브랜치나 작업 브랜치에서 `test_2`로 PR을 올리고, 누구든 병합한다.
2. `test_2`에서 함께 돌려 보며 오류·보완점을 찾는다. 수정은 `test_2`에 직접 하지 않고 개인·작업 브랜치에서 고쳐 다시 PR로 올린다.
3. `test_2`에서 확인된 것을 `test_2` → `test_1` PR로 올리고, 대표가 확인 후 병합한다.
4. `test_1` → `main` PR도 대표가 병합한다.
5. 병합 후 각자 `main`(또는 `test_1`)을 개인 브랜치에 병합해 맞춘다.
6. 배포된 빌드의 급한 수정은 `test_1`에서 가른 작업 브랜치 → `test_1_hotfix` → `test_1`로 올린다(ADR-0021). `test_1`에 들어간 뒤 개인 브랜치에 `test_1`을 병합해 `test_2`로 올려 되맞춘다.

- 대표의 에이전트는 대표의 `gh` 로그인(`chan12392`)으로 돌기 때문에 대표와 같이 취급한다.
- PR 교차 검토(ADR-0006, ADR-0012): 필요할 때 사람이나 `o review <PR> --post`로 검토한다. 검토 자체는 병합의 필수 조건이 아니다. `o review`는 검토 댓글만 남기고 승인이나 병합은 하지 않는다. 병합은 위 표의 규칙을 따른다.
- 병합 방식은 merge commit만 쓴다(squash·rebase 병합은 저장소에서 꺼 두었다). 브랜치 간 이력이 어긋나지 않게 하기 위해서다.
- 비공개 무료 플랜이라 GitHub 브랜치 보호를 켤 수 없다. 그래서 아래 지점에서 막거나 알린다. 검사가 빨간색이면 병합하지 않는다.
  - 에이전트 훅(`o hook`): 보호 브랜치 커밋·push, 대표가 아닌 `gh` 로그인으로 `test_1`·`test_1_hotfix`·`main`에 병합(`gh pr merge`, `gh api` 병합 호출)을 막는다. PR의 base나 로그인 계정을 확인하지 못하면 막는다(base를 모르면 대표도 막는다).
  - git 훅(`o scan`): 보호 브랜치 커밋, 보호 브랜치로의 push를 막는다.
  - CI: `branch-flow.yml`은 흐름에 맞지 않는 PR을 실패시키고, `merge-guard.yml`은 대표가 아닌 사람이 `test_1`·`test_1_hotfix`·`main`에 병합·push하면 실패로 알린다(사후 경보 — 이미 병합된 것을 막지는 못한다. 대표가 되돌린다).
  - 사람이 GitHub 웹 화면의 병합 버튼을 누르는 것은 기술적으로 막을 수 없다. 규칙으로 지킨다.

## 작업 폴더 (Orca 프로젝트 `project_O`)

| 폴더 | 브랜치 | 용도 |
|---|---|---|
| `orca/projects/project_O` | 자기 개인 브랜치 또는 작업 브랜치 | 작업 |
| `orca/workspaces/project_O/test_2` | `test_2` | **읽기 전용 거울.** 병합된 작업을 함께 돌려 보는 곳 |

- 보호 브랜치(`test_1`·`test_2`·`main`)가 체크아웃된 폴더는 에이전트가 바꿀 수 없다. 파일 수정, 허용 목록 밖의 셸 명령(조회·테스트·`git pull --ff-only`만 허용), 커밋, push, O 작업 전달이 모두 막힌다. 다른 폴더의 세션이 경로로 거울 파일을 고치려 해도 막는다.
- 거울은 `test_2`를 따른다. 예약 작업이 5분마다 `origin/test_2`로 fast-forward한다(`o sync-mirror`, 설치는 `scripts/install-mirror-sync.ps1`, 로그 `%LOCALAPPDATA%\project_O\mirror-sync.log`). 동기화는 `test_1`·`test_2`·`main` 체크아웃만 받는다. 로컬 변경이 있으면 지우지 않고 동기화를 멈춘 뒤 로그에 남긴다.
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

- 프로필: `%LOCALAPPDATA%/project_O-agents/<claude|codex|agy|opencode>/home` (`o agents path`). O 상태 폴더(`%LOCALAPPDATA%/project_O`, 보호 대상) 밖에 둔다. 로그인은 CLI마다 한 번 `o agents login <claude|codex|opencode>` (agy는 시스템 로그인을 그대로 씀). OpenCode는 카드만 지원하고 Claude 구독 로그인을 사용하지 않는다(ADR-0015).
- Devin은 설정 폴더를 옮길 수 없어 project_O 프로필을 켠 계획에서 거부한다. 평소 프로필에서 카드만 지원하며 총괄은 아직 지원하지 않는다(ADR-0016).
- 확인: `o agents doctor` — 로그인 상태와 프로필 오염 검사. 수동 테스트: `o agents shell <cli>`. 카드 실행: `o dispatch plan.json`(기본 `--profile project-o`, `--clean`과 같음). 평소 CLI 설정으로는 `--profile own`. 앱의 "project_O 프로필 사용"이 같은 선택이다(ADR-0013).
- 테스트 프로젝트는 홈 폴더 밖에 둔다(Claude는 시작 폴더 위의 지시 파일을 읽는다. 홈 폴더의 것만 프로필이 뺀다).
- Orca 터미널은 자체 훅을 더하므로 `o agents shell`이나 일반 터미널에서 쓴다.
- O 코드는 로그인 파일을 만들거나 읽지 않는다. 로그인 상태는 각 CLI의 상태 명령으로만 본다.

## 데스크톱 셸 (`app/src-tauri`)

- Tauri 2 셸. 크레이트 `project-o`, 실행 파일 `project_O.exe`, 창 제목 "Project O". 빌드: `cd app && npm run tauri build -- --no-bundle` → `target/release/project_O.exe`. 개발: `npm run tauri dev`(Vite 1430 포트, `tauri.conf.json`의 `devUrl`·`devCsp`와 같이 고친다).
- 화면은 프로세스를 직접 띄우지 않는다. 셸의 정해진 Tauri 명령(`app/src-tauri/src/commands/`)만 부르고, 그 명령은 `o-core`를 쓴다. 명령을 더하면 개발 서버 `/api`(`app/dev/liveRuns.ts`)와 `o` 명령에도 같은 동작을 둔다.
- 에이전트 설치는 공식 npm 패키지만 앱에서 실행하고, 나머지는 공식 안내를 연다. 로그인은 새 콘솔 창의 공식 명령으로 한다.
- 설치 파일(NSIS, 사용자별, `o.exe` 사이드카 포함): `pwsh scripts/build-installer.ps1`, `test_1` push마다 `release.yml`이 프리릴리스로 올린다. 설치 폴더·상태 폴더 구분과 안내는 [docs/INSTALL.md](docs/INSTALL.md).
- Linux CI는 WebView 라이브러리가 없어 `cargo test/clippy`에서 `project-o`를 뺀다(`.github/workflows/guard.yml`). Windows 전용 코드의 import·`mut`는 `#[cfg(windows)]`로 묶어 Linux clippy가 통과하게 한다.

## 프로바이더 정책 자동 차단

Antigravity는 공급자 정책에 따라 총괄·카드·예약 실행과 자동 계정·모델·사용량 조회를 보류한다(ADR-0020). 아래의 Antigravity 실행 설명은 재개 전 역사적 기록이다.

사람이 매번 약관을 확인하지 않도록 아래 지점에서 자동으로 막는다. 규칙 원문은 `crates/o-guard/src/rules.rs`, 근거는 `docs/DESIGN.md` §8.

| 지점 | 무엇을 막나 |
|---|---|
| 에이전트 훅 (`.claude/settings.json`, `.codex/hooks.json` → `o hook`, `crates/o-guard`) | 인증 파일·토큰 접근(Read·Grep 포함), 프로바이더 서버 직접 호출, 권한 확인 끄는 옵션이 들어간 명령·코드 수정 / 보호 브랜치(test_1·test_2·main) 커밋·push / 대표 외 test_1·main 병합(`gh pr merge`·`gh api`) / 규칙 밖 브랜치 이름. 훅이나 gh 확인이 실패하면 막는다. `o`가 PATH에 없거나 훅이 오류로 끝나도 막는다: Claude는 `"onFailure": "block"`(Claude Code 2.1.295 이상, 시간 초과도 막음), Codex는 `o hook \|\| exit 2`(Codex는 시간 초과를 통과시킨다) |
| git 훅 (`.githooks/` → `o scan`, `npm install` 시 자동 설정) | 같은 코드 규칙으로 커밋 검사, 보호 브랜치 커밋·push |
| CI (`.github/workflows/guard.yml`, `branch-flow.yml`, `merge-guard.yml`) | 전체 파일 정책 검사 + 테스트 + 타입 검사, PR 방향 검사, 대표 외 test_1·main 병합 경보 (읽기 전용 토큰) |
| O CLI (`crates/o-core/src/policy.rs`) | 카드에 실행 파일·인자·환경변수·계정 지정, 허용 외 프로바이더, 한도에 걸린 서비스 재사용(다른 계정 포함), 동시 실행·하루 실행 상한 초과(카드 시작 직전에도 확인), O 상태 폴더·홈 폴더를 품는 작업 폴더, 정책 위반 내용의 팀 메시지, 카드의 총괄·다른 실행 사칭 (ADR-0002), 오늘 사용 예산을 다 쓴 서비스(`daily-budget`, 하루 사용 예산이 켜져 있을 때만 — 기본 켜짐, `o budget --on\|--off`. 카드 시작 직전에도 확인 — ADR-0004, ADR-0013) |

- YOLO가 꺼진 기본 모드에서 총괄 CLI는 읽기 전용이다(ADR-0008): Claude는 `--tools Read,Grep,Glob`(MCP 없음), Codex는 `-s read-only`, Antigravity는 `--mode plan --sandbox`에 파일 쓰기·셸·MCP·URL 동작을 모두 거부하는 세션 설정(plan 모드만으로는 막히지 않아 거부 규칙이 필수, ADR-0008 개정 1). 계획 승인 전에 프로젝트 훅이 돌지 않게 Claude는 `--setting-sources user --settings {"disableAllHooks":true}`, Codex는 `--disable hooks`, agy는 프로젝트에 `.agents/hooks.json`이 있으면 총괄을 맡지 않는다(개정 2). 총괄 모델은 고르지 않으면 CLI 기본이고, 고르면 카드 `model`과 같은 형식 검사를 거친다. 총괄의 계획은 `o dispatch`와 같은 정책을 두 번(저장할 때·실행할 때) 거치고, 사람이 "실행"을 눌러야 한 번만 전달된다. 첨부는 O 상태 폴더 밖(`%LOCALAPPDATA%/project_O-lead`)에 새 파일로만 저장하고 링크·로그인 정보 위치는 받지 않는다. 총괄 CLI는 API 키 변수(`ANTHROPIC_API_KEY`·`ANTHROPIC_AUTH_TOKEN`·`CODEX_API_KEY`)를 물려받지 않는다.
- YOLO가 켜지면 총괄과 카드가 승인 없이 파일과 명령을 다룬다(ADR-0014). 하루 예산이 켜져 있고 제한에 맞는 계획은 자동 실행한다. O 표식·CLI 거부 규칙·가드로 에이전트의 직접 전달을 제한하지만, 같은 Windows 사용자 권한에서 표식 제거 또는 별도 프로세스를 이용한 우회는 막을 수 없다. Claude YOLO는 O 가드 훅을 주입하며 Codex·Antigravity에는 동일한 훅 적용이 확인되지 않았다.
- 기본 모드의 카드 CLI에 열어 주는 권한은 팀 대화 명령(`o msg`)과 읽기 전용 컨텍스트 검색(`o context`, ADR-0005)뿐이다: Claude는 `--allowedTools "Bash(o msg *)" "Bash(o context *)"`, Codex는 실행 폴더 `--add-dir`(셸 명령은 샌드박스 안에서 그대로 실행), Antigravity는 정확히 `o msg read`·`o msg post` 두 명령(그 CLI의 허용 규칙은 전체 일치만 지원해 `o context`는 열지 않고, 프롬프트에 주입된 컨텍스트만 쓴다). 카드 안의 `o context`는 o.db를 읽기만 하고 `o context add`는 거부한다. 그 밖의 권한 확인은 그대로 둔다.

- 차단되면 우회하지 않는다. 사유를 그대로 상대에게 공유하고, 규칙이 틀렸다면 ADR로 바꾼다.
- `crates/o-guard/src/rules.rs`의 규칙·허용 경로, `crates/o-core/src/policy.rs`의 `LIMITS`(화면 표시용 사본 `src/core/limits.ts`)는 공용 경계다(ADR 기록). 설정·환경변수로 상한을 올리는 길은 일부러 두지 않았다.
- Codex는 프로젝트 훅을 처음 쓸 때와 훅 명령이 바뀔 때 신뢰 승인을 요구할 수 있다. 훅이 안 돌아도 git 훅과 CI가 같은 검사를 한다.

## 커밋·PR

- 커밋 메시지: `<type>: <요약>` (`feat`, `fix`, `docs`, `refactor`, `test`, `chore`).
- PR 본문은 `.github/pull_request_template.md`를 채운다. 무엇을 바꿨는지와 어떻게 확인했는지를 적는다.
