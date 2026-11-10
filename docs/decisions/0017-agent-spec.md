# ADR-0017: 에이전트 명세와 재감지

## 결정

- `AgentSpec` 표에 CLI 식별자, 별칭, 공식 안내, 지원·프로필 상태, 연결된 프로바이더의 헤드리스 고정 인자와 모델 조회 인자를 모은다. 공급자 코드가 이를 읽으며 스트림 파서, 로그인 확인, 정책과 가드는 기존 코드가 책임진다.
- 현재 PATH를 우선하고 Windows 레지스트리의 사용자·시스템 PATH를 다시 읽은 뒤 npm, pnpm, nvm, Volta, Bun과 일반 설치 경로를 탐색한다. 프로세스 PATH를 바꾸지 않는다. 셸 스크립트 셔임은 기존처럼 셸 없이 해석한다.
- 설정의 다른 CLI 목록은 결과를 캐시하고 `다시 감지`에서 경로와 목록을 다시 읽는다. 실패하면 화면에 이전 목록을 유지하며 늦은 응답은 버린다. 감지는 버전 명령만 실행하고 로그인 검사는 별도다.
- Codex 작업 폴더의 신뢰 설정은 O가 만들지 않는다(개정 1, #35). 신뢰하지 않은 프로젝트에서 Codex는 프로젝트 `.codex/`(설정·MCP 서버·훅)를 싣지 않는다(learn.chatgpt.com/docs/config-file/config-basic: "Codex skips project-scoped `.codex/` layers, including project-local config, hooks, and rules."). 신뢰는 사용자가 CLI 절차로 직접 정한다. Git 저장소가 아닌 폴더에서도 돌도록 카드도 총괄처럼 `--skip-git-repo-check`를 쓴다. Claude의 신뢰 파일에는 인증 정보가 섞일 수 있어 건드리지 않는다. 평소 프로필과 Antigravity의 동작은 바꾸지 않는다.
- 미연결 CLI는 감지와 공식 설치 안내만 제공한다. 지원되지 않는 환경과 필수 명령을 확인하며, YOLO 기본값은 계속 꺼짐이다.

## 출처와 검증 범위

Orca 1.4.246의 감지 표·설치 폴더 아이디어를 참고했다. [MIT 라이선스](https://github.com/stablyai/orca/blob/main/LICENSE)와 [저장소 커밋](https://github.com/stablyai/orca/commit/0646d26ff00289b86763acc52e42faeb73a94110)을 확인했으며 저작권과 전문은 `THIRD_PARTY_NOTICES.md`에 있다. 미연결 CLI의 실행 방식은 검증하지 않았다.

## 개정 2 (2026-11-10): Git 저장소에서는 Codex가 스스로 신뢰를 기록한다

### 확인한 것 (codex-cli 0.167.0, Windows, project_O 프로필)

- 카드와 같은 인자(`codex exec --json -s workspace-write -C <폴더> --skip-git-repo-check`)로 **Git 저장소** 안에서 한 턴을 돌리면, 명령을 하나도 실행하지 않은 턴이어도 Codex가 프로필 `config.toml`에 `[projects.'<저장소 루트>']` `trust_level = "trusted"`를 스스로 쓴다.
- 카드별로 분리한 작업 폴더(worktree)에서 돌면 그 폴더가 아니라 **원본 저장소 경로**가 기록된다(실행 `20261110-131428-sqm9`).
- **Git 저장소가 아닌 폴더**는 파일을 써도 기록되지 않는다.
- 확인하지 않은 것: 기록한 그 실행 안에서 이미 `.codex/`가 실리는지, 총괄의 읽기 전용 실행(`app-server`, `exec -s read-only`)도 기록하는지, 다른 Codex 버전·운영체제.

### 결정

- "O는 신뢰 설정을 만들지 않는다"는 그대로다. 다만 "신뢰는 사용자가 CLI 절차로 직접 정한다"는 Git 프로젝트의 Codex 카드에는 맞지 않는다. **Git 프로젝트에서 Codex 카드를 한 번 돌리면 그 프로젝트는 신뢰된 것으로 본다.** 그 뒤의 Codex 카드는 그 저장소의 `.codex/`(설정·MCP 서버·훅)를 싣는다.
- 대표의 판단(2026-11-10): 자기 프로젝트만 올리는 지금 사용에서는 이 자동 신뢰를 유지한다. 저장소의 Codex용 가드 훅(`.codex/hooks.json` → `o hook`)이 Codex 카드에서 돌게 되는 이점이 있다. O가 그 표를 지우거나 막지 않는다.
- 총괄은 계획 승인 전에 프로젝트 훅이 돌지 않도록 전처럼 `--disable hooks`를 쓴다(ADR-0008 개정 2). 이 개정은 그 장치를 바꾸지 않는다.
- Codex 샌드박스 점검 기록의 `configHash`는 이 표를 빼고 계산한다(ADR-0040 개정 2).

### 결과

- 훅은 샌드박스 밖에서 사람의 권한으로 돈다. 남이 만든 Git 저장소를 프로젝트로 등록해 Codex 카드를 돌리면, 그 저장소의 `.codex/` 훅과 MCP 서버가 따로 묻지 않고 실행될 수 있다. 외부 저장소를 다루게 되면 등록할 때 이를 알리는 안내를 더한다(미구현).
- Codex가 이 동작을 바꾸면 위 "확인한 것"을 다시 확인한다.
