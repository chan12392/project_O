# project_O 작업 규칙

사람과 에이전트(Claude Code, Codex 등) 모두 이 규칙을 따른다. Claude Code는 `CLAUDE.md`를 통해 이 파일을 읽는다.

## 팀

| 코드 | 개인 브랜치 |
|---|---|
| A | `younchan` |
| B | `yongwoo` (임시 이름 — 합류 시 본인이 정하면 바꾼다) |

## 브랜치

- `main`에서는 직접 작업하지 않는다. `main`은 `test`를 통과한 결과만 받는다.
- 고정 브랜치는 4개다: `main`, `test`, `younchan`, `yongwoo`. 이 외의 최상위 브랜치를 만들지 않는다.
- 각자 자기 개인 브랜치에서 작업한다. 개인 브랜치에서 한 단계 더 갈라진 작업 브랜치(브랜치의 브랜치)를 허용한다.
  - 이름: `<개인 브랜치>-<주제>` (예: `yongwoo-account-vault`). 작업 브랜치에서 다시 가지를 치지 않는다.
  - `/`로 나누지 않는 이유: git은 `younchan` 브랜치가 있으면 `younchan/<주제>`를 만들 수 없다.
  - 사람마다 동시에 살아 있는 작업 브랜치는 최대 3개. 개인 브랜치에 병합하면 바로 삭제한다.
- 상대의 개인 브랜치·작업 브랜치에 커밋하지 않는다.
- 개인 브랜치 목록은 `crates/o-guard/src/hook.rs`의 `PERSONAL_BRANCHES`와 `.github/workflows/branch-flow.yml`에 함께 적혀 있다. 바꿀 때 두 곳을 같이 고친다.

## 병합 흐름

```
younchan-<주제> ──PR──▶ younchan ──PR──▶ test ──(통합 수정)──PR──▶ main
yongwoo-<주제>  ──PR──▶ yongwoo  ──PR──▶ test ─┘
```

1. 작업 브랜치 → 자기 개인 브랜치: 본인이 병합한다.
2. A, B는 각자 개인 브랜치에서 `test`로 PR을 올린다.
3. `test`에 병합한 뒤 함께 돌려 보며 오류·보완점을 찾는다. 수정은 `test`에 직접 하지 않고 개인 브랜치에서 고쳐 다시 PR로 올린다.
4. 두 사람 모두 승인하면 `test` → `main` PR을 병합한다. PR 작성자는 템플릿의 승인 체크로, 상대는 GitHub 리뷰 Approve로 승인한다.
5. `main` 병합 후 각자 `main`을 개인 브랜치에 병합해 맞춘다.

- 병합 방식은 merge commit만 쓴다(squash·rebase 병합은 저장소에서 꺼 두었다). 브랜치 간 이력이 어긋나지 않게 하기 위해서다.
- 위 흐름에 맞지 않는 PR은 `.github/workflows/branch-flow.yml` 검사가 실패시킨다. 비공개 무료 플랜이라 브랜치 보호를 켤 수 없어 검사 실패가 병합 버튼을 막지는 않는다. 검사가 빨간색이면 병합하지 않는다.

## 작업 폴더 (Orca 프로젝트 `project_O`)

| 폴더 | 브랜치 | 용도 |
|---|---|---|
| `orca/projects/project_O` | 자기 개인 브랜치 (`younchan` 또는 `yongwoo`) | 작업 |
| `orca/workspaces/project_O/test` | `test` | **읽기 전용 거울.** 병합 결과를 받아 함께 돌려 보는 곳 |

- `test`·`main`이 체크아웃된 폴더는 에이전트가 바꿀 수 없다. 파일 수정, 허용 목록 밖의 셸 명령(조회·테스트·`git pull --ff-only`만 허용), 커밋, push, O 작업 전달이 모두 막힌다. 다른 폴더의 세션이 경로로 거울 파일을 고치려 해도 막는다.
- 거울은 예약 작업이 5분마다 `origin/test`로 fast-forward한다(`o sync-mirror`, 설치는 `scripts/install-mirror-sync.ps1`, 로그 `%LOCALAPPDATA%\project_O\mirror-sync.log`). 로컬 변경이 있으면 지우지 않고 동기화를 멈춘 뒤 로그에 남긴다.
- 예약 작업은 저장소 밖에 설치된 `o.exe`를 실행한다(거울이 뒤처져도 동기화가 돌도록). 동기화의 git 호출은 저장소 git 훅을 쓰지 않는다.
- 각자 PC에서 한 번 설정한다: `cargo install --path crates/o-cli` → `git worktree add <거울 폴더> test` → `pwsh scripts/install-mirror-sync.ps1 -Mirror <거울 폴더>`.

## 설계 변경

- 두 사람의 방향(관제 중심 / 데스크톱 통합 작업대)이 다르다. 공용 경계에 영향을 주는 결정은 `docs/decisions/`에 ADR로 남기고 두 사람이 승인한 뒤 구현한다.
  - 공용 경계: 기술 스택, 공용 데이터 모델(세션·계정·프로바이더), 프로바이더 연결 방식, 라우팅 단위, 화면 최상위 구조.
- 자기 영역 안의 구현 결정은 ADR 없이 진행한다.
- 현재 설계 초안과 미결정 목록은 `docs/DESIGN.md`에 있다.

## 보안

- 구독 계정 인증 정보·토큰·`.env`는 커밋하지 않는다. 계정 데이터는 각자 PC에만 둔다.
- 테스트·데모 데이터에 실제 계정명·이메일을 넣지 않는다.

## 프로바이더 정책 자동 차단

사람이 매번 약관을 확인하지 않도록 아래 지점에서 자동으로 막는다. 규칙 원문은 `crates/o-guard/src/rules.rs`, 근거는 `docs/DESIGN.md` §8.

| 지점 | 무엇을 막나 |
|---|---|
| 에이전트 훅 (`.claude/settings.json`, `.codex/hooks.json` → `o hook`, `crates/o-guard`) | 인증 파일·토큰 접근(Read·Grep 포함), 프로바이더 서버 직접 호출, 권한 확인 끄는 옵션이 들어간 명령·코드 수정 / main 커밋·push / 규칙 밖 브랜치 이름. 훅이 실패하면 막는다 |
| git 훅 (`.githooks/` → `o scan`, `npm install` 시 자동 설정) | 같은 코드 규칙으로 커밋 검사, main 커밋·push |
| CI (`.github/workflows/guard.yml`) | 전체 파일 정책 검사 + 테스트 + 타입 검사 (읽기 전용 토큰) |
| O CLI (`crates/o-core/src/policy.rs`) | 카드에 실행 파일·인자·환경변수·계정 지정, 허용 외 프로바이더, 한도에 걸린 서비스 재사용(다른 계정 포함), 동시 실행·하루 실행 상한 초과(카드 시작 직전에도 확인), O 상태 폴더·홈 폴더를 품는 작업 폴더, 정책 위반 내용의 팀 메시지, 카드의 총괄·다른 실행 사칭 (ADR-0002) |

- 카드 CLI에 열어 주는 권한은 팀 대화 명령(`o msg`)뿐이다: Claude는 `--allowedTools "Bash(o msg *)"`, Codex는 실행 폴더 `--add-dir`, Antigravity는 정확히 `o msg read`·`o msg post` 두 명령(그 CLI의 허용 규칙은 전체 일치만 지원). 그 밖의 권한 확인은 그대로 둔다.

- 차단되면 우회하지 않는다. 사유를 그대로 상대에게 공유하고, 규칙이 틀렸다면 ADR로 바꾼다.
- `crates/o-guard/src/rules.rs`의 규칙·허용 경로, `crates/o-core/src/policy.rs`의 `LIMITS`(화면 표시용 사본 `src/core/limits.ts`)는 공용 경계다(ADR + 두 사람 승인). 설정·환경변수로 상한을 올리는 길은 일부러 두지 않았다.
- Codex는 프로젝트 훅을 처음 쓸 때 신뢰 승인을 요구할 수 있다. 훅이 안 돌아도 git 훅과 CI가 같은 검사를 한다.

## 커밋·PR

- 커밋 메시지: `<type>: <요약>` (`feat`, `fix`, `docs`, `refactor`, `test`, `chore`).
- PR 본문은 `.github/pull_request_template.md`를 채운다. 무엇을 바꿨는지와 어떻게 확인했는지를 적는다.
