# ADR-0035: CLI 인증 환경변수 전달

- 상태: 채택 (2026-11-09 개정 1: API 과금 모드 선택)
- 날짜: 2026-11-09
- 관련: [ADR-0008](0008-lead-chat.md), [ADR-0013](0013-profile-and-budget-switches.md), [ADR-0015](0015-opencode.md), [인증 조사](../research/agent-chat-and-pstack.md)

## 배경

O는 구독 대신 API로 과금되는 일을 막으려고 자식 CLI에서 인증 환경변수를 제거해 왔다. [Anthropic의 제품 내 Claude Code 제공 조건](https://code.claude.com/docs/en/legal-and-compliance)은 Claude Code를 게시된 그대로 설치·실행하고 Claude 계정과 사용자 자신의 API 키를 포함한 내장 인증 방법을 제거·비활성화·제한하지 않도록 요구한다. 별도 합의가 없다면 해당 제품 제공에는 상업 약관과 그 밖의 조건도 적용된다. 환경변수 제거가 이 조항에 해당하는지는 확인되지 않았으며 기존 동작을 위반으로 단정하지 않는다.

> Claude Code must be installed and run as published by Anthropic, and customers may not remove, disable, or restrict any authentication method built into it (including methods that permit signing in with a Claude account or the user's own API key).

참고한 Orca(커밋 48ccdd5)는 경로마다 다르다. 채팅 실행은 사용자 셸에서 읽은 환경을 그대로 쓴다([`claude-structured-child-env.ts:85`](https://github.com/stablyai/orca/blob/48ccdd5212ea2a744a75efb32232b9104214bd76/src/main/claude/structured/launch/claude-structured-child-env.ts#L85) "Merge onto the shell environment to retain PATH and ambient Anthropic auth"). 터미널 실행은 사용자 셸 안에서 CLI를 띄운다. 반면 Orca 앱 자신의 프로세스 환경을 넘기는 경로([`claude-child-process-environment.ts:47-83`](https://github.com/stablyai/orca/blob/48ccdd5212ea2a744a75efb32232b9104214bd76/src/main/claude/claude-child-process-environment.ts#L47-L83))와 버전 확인은 인증 변수를 뺀다. 사용자가 TUI에서 이미 거절한 키는 채팅 실행에서도 뺀다(`claude-rejected-api-key.ts`). 즉 사용자가 셸에 둔 키는 넘긴다. Windows의 O는 사용자 환경을 그대로 물려받으므로 O에서는 "지우지 않음"이 이에 해당한다. 거절한 키를 가려내려면 계정 정보가 함께 든 Claude 설정 파일(`~/.claude.json`)과 키 값을 읽어야 한다. O는 CLI의 로그인 관련 파일과 키 값을 읽지 않는다는 원칙(AGENTS.md "깨끗한 에이전트")을 지키려고 그 부분은 따르지 않고 경고로 대신한다.

## 결정

- O는 각 CLI의 인증 선택을 CLI와 사용자에게 맡긴다. 일반 프로필의 총괄·카드와 상태·사용량·모델 조회를 포함한 CLI 실행에서 API 인증 환경변수를 제거하지 않는다. 이 결정은 Antigravity 실행·자동 조회 보류(ADR-0020)나 다른 공급자의 사용 정책을 해제하지 않는다.
- 깨끗한 프로필은 기존 환경 허용 목록을 유지하면서 실행할 CLI가 사용하는 것으로 명시된 인증 변수만 추가로 전달한다. Claude에는 `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`을 포함한다. 목록 밖 인증 변수와 임의 환경 전체를 허용하지 않는다. Claude 구독 OAuth 토큰은 기존 `claude-oauth-token` 가드가 차단하므로 깨끗한 프로필의 허용 목록에 넣지 않았다. CLI 간에 쓰는 변수 이름이 같을 수 있다. 프로필별 로그인·설정 격리는 유지한다.
- 미결: 깨끗한 프로필은 `CLAUDE_CODE_OAUTH_TOKEN`을 넘기지 않는다(o-guard `claude-oauth-token` 규칙). 평소 설정 프로필은 셸 환경을 그대로 물려받는다.
- 인증 변수의 존재는 이름만 확인해 API 과금 가능성을 경고한다. O는 경고를 위해 값이나 인증 파일을 읽거나 출력·저장하지 않는다. 자식 CLI는 전달받은 환경과 자체 설정에 따라 인증한다.
- 환경에 키가 있으면 CLI의 인증 우선순위에 따라 구독 대신 API 계정이나 크레딧에 비용이 청구될 수 있다. [Claude 인증 우선순위](https://code.claude.com/docs/en/authentication)에서 클라우드 자격 증명이나 `ANTHROPIC_AUTH_TOKEN` 등이 `ANTHROPIC_API_KEY`보다 먼저 선택될 수 있으므로 키의 존재만으로 실제 인증이나 요금을 확정하지 않는다. 사용자는 실행 환경과 공급자의 인증·과금 설정을 확인한다.
- 이 결정은 SDK 통합 허용이나 구독·배포 조건의 적합성을 보증하지 않는다. 공급자별 약관 확인과 OpenCode의 Claude 구독 연결 금지(ADR-0015)는 그대로다.

## 이유

CLI의 인증 선택을 사용자가 제어하면서 환경에 남은 키로 인한 과금 가능성을 알리기 위해서다.

## 결과

가짜 인증 변수로 일반·깨끗한 프로필, 총괄·카드, 상태·사용량·모델 조회에 전달되는 범위와 공급자 간 격리를 확인한다. 경고에는 이름의 존재만 쓰이며 값이 출력·기록되지 않는지 검사한다. 실제 계정이나 API 과금 요청은 테스트에 쓰지 않는다.

## 개정 1 (2026-11-09): API 과금 모드 선택

검토에서 경고만으로는 구독만 쓰려는 사람이 API 과금을 피할 수 없다는 지적이 있어, 사람이 과금 방식을 명시적으로 고르게 한다.

### 결정

- O 상태 폴더의 `settings.json`에 `apiBillingMode`를 둔다(하루 예산·RAM 상한과 같은 파일). 값은 둘이다.
  - `warn`(경고만, 기본): 위 결정 그대로다. 인증 변수를 넘기고 이름만 경고한다. 값이 없거나 알 수 없는 값이면 `warn`이다.
  - `subscription`(구독만): O가 에이전트 작업으로 CLI를 띄우기 직전, 그 CLI에 넘길 환경에 그 CLI가 쓰는 인증 변수(`auth_env_names`)가 있으면 실행을 시작하지 않는다. 거부 사유에는 변수 이름과 진행 방법(환경에서 변수를 지우고 O를 다시 시작하거나 경고만 모드로 바꾸기)을 적는다.
- 판단은 실제로 넘길 환경으로 한다. 평소 설정 프로필은 O가 물려받은 환경, 깨끗한 프로필은 허용 목록을 거친 환경에 실행별 변경을 적용한 결과다. 변수 이름만 보고 값은 읽거나 출력·저장하지 않는다. 다른 CLI의 변수는 따지지 않는다(예: Codex 카드는 `ANTHROPIC_API_KEY`가 있어도 시작한다).
- 적용 범위(에이전트 작업): 카드(`o dispatch`, 총괄 분배, 예약 실행이 시작한 카드), 후속 지시, `o review` 카드, 총괄 턴(일반·YOLO·카드 질문·전달·검토 턴, Codex app-server 포함), 그래프 Deep 분석. 카드는 실패로 끝나고 사유가 카드 결과에 남는다. 총괄 턴은 오류 이벤트로 남는다. 예약 실행도 같은 총괄 턴·카드 경로를 거친다.
- 적용하지 않는 것: 설치·버전·로그인 상태·사용량·모델 목록 조회와 `o agents login`·`o agents shell`. 모델 턴이 없는 읽기 전용 조회이거나 사람이 직접 모는 공식 명령이라 두 모드 모두 지금처럼 돈다.
- 설정: `o billing [--json] [--warn|--subscription]`, 앱 설정 → 에이전트 → API 과금(경고만/구독만). 카드·총괄 안(`O_CARD_ID`·`O_AGENT_ROLE`)에서는 읽기만 되고 바꾸기는 거부한다. 과금 방식은 사람이 고른다.

### 인증 방법 조항과의 관계

구독만 모드에서도 O는 환경변수를 지우거나 바꾸지 않고, CLI에 인자나 설정을 더해 인증 방법을 끄지 않는다. CLI는 실행되면 게시된 그대로 자신의 인증 우선순위를 따른다. O는 사람이 고른 과금 조건에 맞지 않는 실행을 시작하지 않을 뿐이며, 이는 하루 예산이나 동시 실행 상한으로 카드를 시작하지 않는 것과 같은 O 자신의 실행 정책이다. 사람은 변수를 지운 환경이나 경고만 모드로 언제든 API 키 인증을 그대로 쓸 수 있다. 같은 이유로 "변수를 빼고 실행" 같은 선택지는 두지 않았다.

### 한계

- 인증 변수 목록은 각 CLI가 쓰는 것으로 명시된 이름뿐이다. 클라우드 자격 증명, CLI 설정 파일의 키, `CLAUDE_CODE_OAUTH_TOKEN` 같은 목록 밖 방식은 보지 않으므로 구독만 모드가 API 과금이 없음을 보증하지 않는다.
- Windows에서 사용자 환경변수를 지워도 이미 떠 있는 O 앱은 예전 환경을 물려받고 있다. 앱을 다시 시작해야 반영된다.
