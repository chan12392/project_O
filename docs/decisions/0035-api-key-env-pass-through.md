# ADR-0035: CLI 인증 환경변수 전달

- 상태: 채택
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
