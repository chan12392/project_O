# ADR-0040: project_O 프로필의 Codex는 샌드박스 설정을 마칠 때까지 카드를 맡지 않는다

- 상태: 채택
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
- **한 번에 설정**: 설정 → 에이전트에서 Codex 줄이 "샌드박스 설정이 필요합니다"로 보이고 "Codex 샌드박스 설정" 버튼이 나온다(`AgentStatus::profile_not_ready`). 버튼은 로그인과 같은 길로 새 콘솔 창에서 Codex를 project_O 프로필로 대화형으로 연다(Tauri `agent_setup`, 개발 서버 `/api/agents/<id>/setup`, `o agents open-setup <id>`). 사람은 Codex의 안내대로 샌드박스 설정을 마친다. 터미널에서는 `o agents shell codex`도 같다. O는 그 설정을 직접 쓰지 않는다.
- **바뀌지 않는 것**: 내 설정(`--profile own`)과 YOLO(샌드박스 없이 실행)에서는 거부하지 않는다. Windows가 아닌 곳에서도 거부하지 않는다. 총괄 자체의 Codex 실행(읽기 전용)도 그대로다.

## 결과

- 설정이 없는 Codex 카드가 "done"으로 끝나 아무 일도 하지 않는 일이 없어진다. 거부 사유가 설정 방법을 알려 준다.
- 총괄은 처음부터 쓸 수 있는 에이전트로 계획을 세운다.
- 사람은 Codex 샌드박스 설정을 한 번 해야 project_O 프로필에서 Codex 카드를 쓸 수 있다.
- Codex가 설정 파일 형식을 바꾸면 준비 검사(`WINDOWS_SANDBOX` 정규식)도 함께 고쳐야 한다. 실제로 설정됐는데 거부되면 `o agents doctor`와 앱의 같은 문구로 드러난다.
