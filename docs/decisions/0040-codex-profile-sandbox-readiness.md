# ADR-0040: project_O 프로필의 Codex는 샌드박스 설정을 마칠 때까지 카드를 맡지 않는다

- 상태: 채택. 같은 날 개정 1: 준비 상태의 근거를 설정 줄에서 실제 명령 점검 기록으로 바꿈(아래 "개정 1")
- 날짜: 2026-11-10
- 관련: [ADR-0013](0013-profile-and-budget-switches.md), [ADR-0014](0014-yolo-mode.md), [ADR-0015](0015-opencode.md)

## 배경

실행 `20261109-123716-tent`의 Codex 카드 `core`가 26초 만에 "done"으로 끝났다. 결과는 "바꾼 파일 없음"이었다. Codex의 stderr에는 셸 명령마다 `exec_command failed ... rejected: blocked by policy`가 남았고, 샌드박스 로그는 `Windows sandbox selection: disabled`였다.

- 원인: project_O 프로필의 Codex(`%LOCALAPPDATA%/project_O-agents/codex/home/.codex`, `CODEX_HOME`)에는 Windows 샌드박스 선택(`[windows] sandbox`)이 든 `config.toml`이 없다. 그러면 카드의 고정 인자 `codex exec -s workspace-write`는 Windows에서 명령도 파일 수정도 하지 못한다. 사람의 평소 Codex 프로필은 대화형으로 한 번 설정을 마쳐 그 줄이 있어 잘 돈다.
- `o agents doctor`는 이미 이 상태를 경고했다. 그러나 계획이 Codex를 쓰는 것을 막는 곳이 없었다. 카드는 아무 일도 하지 않고 "done"으로 끝났고, 그 카드에 기대는 카드는 빈 결과 위에서 시작했다.
- 대표의 요구: "가드 때문에 작업이 막히는 일이 없어야 해." 조용히 아무것도 안 하고 끝나는 카드도 막힌 작업이다.

## 선택지

1. O가 `config.toml`에 `[windows] sandbox = "unelevated"`를 쓴다. 690c346에서 이미 확인했듯 새 프로필은 그 뒤 Codex의 샌드박스 준비에서 멈춘다.
2. 카드 실행에 `-c windows.sandbox="unelevated"`를 붙인다. 새 프로필에서 확인한 결과, 첫 샌드박스 명령이 끝없이 멈춘다.
3. 설정이 끝나지 않은 프로필의 Codex 카드는 시작하지 않는다. 총괄에게 미리 알려 다른 에이전트로 계획하게 하고, 사람에게 한 번에 설정을 마칠 길을 준다.

## 결정

3을 고른다.

- **준비 상태는 프로바이더가 안다**: `Provider::profile_not_ready(home)`(기본 `None`). Codex는 Windows에서 프로필의 `config.toml`에 샌드박스 선택이 없으면 사유(`codex::SANDBOX_NOT_SET`)를 돌려준다. `agents::clean::profile_not_ready(root, provider)`가 프로필 위치를 붙여 부른다. `o agents doctor`의 경고도 같은 문구다.
- **계획 거부**: `policy::check_plan_in(plan, ctx, mode)`가 project_O 프로필이고 YOLO가 꺼져 있으면 그 카드를 규칙 `profile-not-ready`로 거부한다. 프로필 위치는 `PolicyContext::profile_not_ready`가 가진다(`store::policy_context`는 `clean::default_root()`, 테스트는 임시 폴더). `check_plan_for(plan, ctx, profile)`는 YOLO가 꺼진 것으로 보고 같은 검사를 한다. `o dispatch`·`o review`·총괄의 계획 표시·분배·자동 실행·러너의 재검사는 실행 모드를 알고 `check_plan_in`을 쓴다.
- **시작 직전 재확인**: 러너는 카드를 띄우기 직전(`run/runner/card.rs`)에 다시 확인한다. 설정이 없으면 그 카드는 CLI를 띄우지 않고 사유와 함께 `failed`로 끝난다. "done"으로 끝나지 않으므로, 기대는 카드도 시작하지 않는다.
- **총괄에게 미리 알림**: project_O 프로필이고 YOLO가 꺼진 총괄 턴에는, 준비되지 않은 에이전트와 사유가 매 턴 한 줄씩 들어간다(`lead::prompt::UNREADY_NOTE`, `PromptInput::unready`). 총괄은 그 provider에 카드를 배정하지 않고 다른 provider(예: claude)로 계획한다. 세션 첫 턴에만 가는 역할 문구는 바꾸지 않았다. 사람이 설정을 마치면 다음 턴부터 그 줄이 사라진다.
- **한 번에 설정**: 설정 → 에이전트에서 Codex 줄이 "샌드박스 설정이 필요합니다"로 보이고 "Codex 샌드박스 설정" 버튼이 나온다(`AgentStatus::profile_not_ready`). 버튼은 로그인과 같은 길로 새 콘솔 창에서 Codex를 project_O 프로필로 대화형으로 연다(Tauri `agent_setup`, 개발 서버 `/api/agents/<id>/setup`, `o agents open-setup <id>`). 사람은 Codex의 안내대로 샌드박스 설정을 마친다. 터미널에서는 `o agents shell codex`도 같다. O는 그 설정을 직접 쓰지 않는다. (개정 1에서 창 없는 설정·점검으로 바뀜)
- **바뀌지 않는 것**: 내 설정(`--profile own`)과 YOLO(샌드박스 없이 실행)에서는 거부하지 않는다. Windows가 아닌 곳에서도 거부하지 않는다. 총괄 자체의 Codex 실행(읽기 전용)도 그대로다.

## 결과

- 설정이 없는 Codex 카드가 "done"으로 끝나 아무 일도 하지 않는 일이 없어진다. 거부 사유가 설정 방법을 알려 준다.
- 총괄은 처음부터 쓸 수 있는 에이전트로 계획을 세운다.
- 사람은 Codex 샌드박스 설정을 한 번 해야 project_O 프로필에서 Codex 카드를 쓸 수 있다.
- Codex가 설정 파일 형식을 바꾸면 준비 검사(`WINDOWS_SANDBOX` 정규식)도 함께 고쳐야 한다. 실제로 설정됐는데 거부되면 `o agents doctor`와 앱의 같은 문구로 드러난다.

## 개정 1 (2026-11-10): 설정 줄이 아니라 실제 명령 점검이 근거다

### 배경

- 이 PC에서 Codex의 공식 헤드리스 설정(`codex app-server`의 `windowsSandbox/setupStart`, `unelevated`)을 project_O 프로필에 돌렸다. 설정은 성공했고 `config.toml`에 `[windows] sandbox = "unelevated"`가 들어갔으며, `windowsSandbox/readiness`도 `ready`를 돌려주었다.
- 그런데도 그 프로필의 `codex exec --json -s workspace-write …`는 첫 셸 명령에서 끝없이 멈췄다. 샌드박스 로그에는 `START: …`만 남고 자식 프로세스가 만들어지지 않았다. 열린 Codex 버그 https://github.com/openai/codex/issues/50725 와 같은 증상이다. 사람의 평소 프로필은 잘 돈다.
- 그래서 설정 줄도, Codex 자신의 준비 상태도 카드가 명령을 실행할 수 있다는 증거가 아니다. 명령을 실제로 실행해 보는 것만이 증거다.

### 결정

- **점검(`providers::codex_sandbox::check`, `agents::check_setup`)**: 사람이 요청할 때만 돈다(설정 → 에이전트의 "Codex 샌드박스 점검", `o agents setup codex`, Tauri `agent_setup`, 개발 서버 `/api/agents/codex/setup`). 자동으로는 돌지 않으며, 에이전트 안(`O_AGENT_ROLE`·`O_CARD_ID`)에서는 거부한다. 창을 띄우지 않는다.
  1. 카드와 같은 해석된 CLI와 깨끗한 환경(`clean::command`)으로 `codex --version`을 읽는다.
  2. `codex app-server`에 `initialize` → `initialized` → `windowsSandbox/readiness`를 묻고, `notConfigured`·`updateRequired`면 `windowsSandbox/setupStart`(`unelevated`)를 보내 `windowsSandbox/setupCompleted`까지 최대 5분 기다린다. 실패하면 Codex가 준 오류 문구로 실패를 기록한다.
  3. 프로필 홈 옆(`<에이전트 루트>/codex/sandbox-probe-<id>`, 프로필 홈·O 상태 폴더 밖)에 새 폴더를 만들고, 카드의 고정 인자 그대로 `codex exec --json -s workspace-write -C <그 폴더> --skip-git-repo-check -`를 띄워 `cmd /c "echo o-sandbox-ok> probe.txt"` 하나만 실행하게 한다. **작은 모델 호출이 한 번 든다.** 최대 120초이며, 넘기면 프로세스 트리를 끝낸다(`kill_tree`와 job 객체). `probe.txt`에 `o-sandbox-ok`가 있을 때만 통과다. 구독만 과금 모드에서는 카드처럼 거부한다.
- **기록**: 결과를 `<에이전트 루트>/codex/sandbox-check.json`에 원자적으로 쓴다(`ok`, `version`, `configHash` — 점검 뒤 `config.toml`의 SHA-256, `checkedAt`, 한국어 `detail`). 시간 초과는 "Codex가 샌드박스 안에서 첫 명령을 시작하기 전에 멈췄습니다"와 위 버그 링크를 남긴다.
  - 위치를 O 상태 폴더가 아니라 프로필 옆에 둔 까닭: 기록은 그 프로필의 성질이고, 프로필을 지우면 함께 사라진다. 준비 검사의 경로 주입(`clean::profile_not_ready(root, …)`, 테스트의 임시 루트)을 그대로 쓴다. CODEX_HOME(`home/.codex`) 밖이라 Codex는 읽지 않는다.
- **준비 상태 = 맞는 성공 기록**: Windows·project_O 프로필·YOLO 꺼짐에서 Codex는 기록이 `ok`이고 그 `version`과 `configHash`가 지금의 `codex --version`(프로세스당 1분 보관)·`config.toml`과 같을 때만 준비됨이다(`codex_sandbox::decide`). 아니면 사유를 돌려준다: 설정 줄 없음(`SANDBOX_NOT_SET`, 가장 싼 첫 검사), 점검 안 함(`NOT_CHECKED`), 점검 실패(`failed_reason` — 기록의 detail), 점검 뒤 Codex나 설정이 바뀜(`CHANGED`). 계획 거부·카드 시작 직전 거부·총괄 프롬프트 줄·doctor·설정 화면은 모두 `Provider::profile_not_ready` 하나를 거친다.
- **버튼**: "Codex 샌드박스 점검"은 새 콘솔 창 대신 위 점검을 돌리고, 도는 동안 "점검하는 중"과 진행 막대를, 끝나면 통과 또는 실패 사유를 보이고 에이전트 상태를 다시 읽는다. 예전의 `o agents open-setup`(새 콘솔 창)은 없앴다.

### 결과

- 이 PC처럼 Codex가 첫 명령에서 멈추는 동안 Codex 카드는 시작하지 않고, 총괄은 다른 에이전트로 계획한다. Codex가 고쳐지면 다시 점검해 통과하는 순간부터 쓴다.
- Codex를 업데이트하거나 프로필 `config.toml`이 바뀌면 다시 점검해야 한다. Codex가 스스로 `config.toml`을 고치는 경우에도 재점검을 요구한다.
- 점검마다 작은 모델 호출 한 번이 든다. 사람이 누를 때만 든다.

## 개정 2 (2026-11-10): 멈춤의 원인은 TEMP였다 — 프로필에 자기 임시 폴더를 준다

### 배경

개정 1은 멈춤을 열린 Codex 버그(openai/codex#50725)로 보고 고쳐질 때까지 기다리기로 했다. 같은 PC에서 조건을 하나씩 바꿔 실제 명령(`cmd /c "echo o-sandbox-ok> probe.txt"`)을 돌려 원인을 좁혔다.

| 조건 | 결과 |
|---|---|
| 평소 환경변수 그대로, `CODEX_HOME`만 project_O 프로필 | 멈춤 |
| 같은 작업 폴더, 잘 도는 다른 `CODEX_HOME` | 통과(약 10초) |
| project_O 프로필 + 잘 도는 쪽의 설정을 `-c`로 모두 적용 | 멈춤 |
| project_O 프로필 + 샌드박스 도우미 실행 파일 복사 | 멈춤 |
| project_O 프로필의 `codex sandbox <명령>`(모델 호출 없음) | 통과 |
| project_O 프로필 + `TEMP`·`TMP`를 빈 새 폴더로 | **통과(11초)** |

- 깨끗한 환경의 허용 목록도, 설정 파일도 원인이 아니다.
- Codex의 Windows 샌드박스는 `CODEX_HOME`마다 쓰기 가능 폴더별 식별자(`cap_sid`의 `writable_root_by_path`)를 만들고, 첫 명령 전에 그 식별자에 `TEMP` 쓰기 권한을 준다. Windows는 이 권한을 폴더 안의 모든 항목에 적용한다.
- 이 PC의 `TEMP`는 최상위 항목만 95,414개다. 잘 도는 `CODEX_HOME`의 식별자는 이미 `TEMP`의 권한 목록에 있었고, project_O 프로필의 식별자는 없었다. 권한 적용이 카드나 점검의 제한 시간 안에 끝나지 못하고 매번 처음부터 다시 시작한 것이다. 멈춘 동안 Codex 아래에는 셸 프로세스가 하나도 없었다.

### 결정

- `Provider::clean_temp(home)`(기본 `None`)을 둔다. Codex는 Windows에서 프로필 홈 옆의 `tmp`(`<에이전트 루트>/codex/tmp`)를 돌려준다. `clean::clean_env_from`은 그 값이 있으면 부모의 `TEMP`·`TMP`를 버리고 이 폴더로 바꾼다. 폴더는 `prepare_profile`이 만든다.
- 다른 프로바이더와 내 설정(`--profile own`) 실행의 `TEMP`는 그대로다.
- 준비 상태의 근거는 개정 1 그대로 실제 명령 점검 기록이다. 이 변경 뒤 이 PC의 `o agents setup codex`가 codex-cli 0.167.0에서 통과했다.

### 결과

- project_O 프로필의 Codex 카드가 Codex 수정을 기다리지 않고 돈다.
- 샌드박스 안의 명령이 사람의 `TEMP`에 쓰지 못한다. 카드의 임시 파일은 프로필의 `tmp`에 쌓이므로 정리 대상이 하나 는다.
- 내 설정으로 도는 Codex는 그 `CODEX_HOME`이 `TEMP` 권한 적용을 한 번 끝낸 적이 있어야 한다. O는 그 프로필을 건드리지 않는다.
- 같은 날 덧붙임: Codex는 카드가 새 프로젝트에서 돌 때 `config.toml`에 `[projects.'<폴더>']` 표를 스스로 더한다(실행 `20261110-131428-sqm9`). 이 표까지 해시에 넣으면 프로젝트마다 첫 카드 뒤에 재점검을 요구하게 되므로, `configHash`는 `[projects…]` 표를 뺀 내용으로 계산한다. 그 밖의 변경은 전처럼 재점검을 요구한다. 예전 방식으로 남긴 기록은 한 번 다시 점검해야 한다.
