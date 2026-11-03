# 에이전트 대화 연결과 pstack 조사

조사일: 2026-11-03. 기준: Project O `5fdcaa6`, Orca `4e3ee00`.

## Orca Chat UI와 O의 차이

설치된 Orca의 `resources/app.asar`에서 `out/shared/native-chat/native-chat-providers.js`와 `native-chat-streaming.js`를 확인했다. Orca는 기본 화면을 채팅 또는 터미널로 선택하고 에이전트별로 덮어쓸 수 있다. 진행 중인 답변 미리보기를 최종 기록과 대조해 중복을 숨긴다. 설치 패키지에는 Claude Agent SDK도 포함되어 있다. 패키지 포함 사실만으로 특정 계정의 이용 조건이 충족된다고 판단할 수는 없다.

O는 총괄이 대화를 계획으로 바꾸고 정책 검사 후 작업카드로 분배하는 흐름에 집중한다. 기본 총괄은 읽기 전용이며 기존에는 Codex의 `exec --json` 결과가 끝난 뒤 답변을 표시했다. 이번 변경으로 일반 Codex 총괄의 전송·재개와 답변 미리보기에 app-server를 쓰지만, 승인·질문 UI는 아직 없다. 작업카드는 계속 기존 CLI를 사용한다. 따라서 연결 방식이 같아진 부분과 사용자에게 보이는 기능을 구분해야 한다.

O에 우선 필요한 개선은 명시적 승인·질문과 다른 프로바이더의 진행 중 답변·안전한 재개를 공식 프로토콜로 연결하는 것이다. 기존 O의 계획 검사·예산·프로필 경계를 유지하고, 한 에이전트의 구현 가능성을 다른 에이전트의 정책 허용으로 일반화하지 않는다.

## 프로바이더 경계

| 대상 | 확인된 공식 경로와 조건 | O에서의 판단 |
| --- | --- | --- |
| Codex | [app-server](https://learn.chatgpt.com/docs/app-server)는 로컬 프로세스의 대화 생성·재개, 스트리밍, 승인 요청 및 중지를 제공한다. [Sign in with ChatGPT](https://developers.openai.com/siwc/token-sharing-open-source)는 별도의 동의와 조건을 둔다. | 공식 CLI가 소유한 로그인과 기존 프로필·권한 경계를 유지하며 app-server를 연결한다. O가 토큰을 읽거나 전달하지 않는다. |
| Claude | [Agent SDK](https://code.claude.com/docs/en/agent-sdk/overview)와 수정하지 않은 Claude Code가 공식 경로다. [법률 안내](https://code.claude.com/docs/en/legal-and-compliance)는 제품 내 수정 없는 CLI와 사용자 직접 인증을 허용하는 조건을 설명하면서, 제품 개발자의 SDK 인증에는 API 키를 권고하고 사용자 구독 자격 증명을 대신 전달하는 것을 금지한다. [구독 안내](https://support.claude.com/en/articles/15036540-use-the-claude-agent-sdk-with-your-claude-plan)는 사용자 플랜으로 로그인한 SDK 및 서드파티 앱의 사용량을 설명한다. | SDK 사용 시에도 O가 로그인 정보를 수집·중개하지 않는다. 구독 사용 및 배포 조건은 두 문서의 적용 범위를 구분해 확인하고, API 과금으로 자동 전환하지 않는다. |
| Antigravity | [FAQ](https://antigravity.google/docs/faq)는 소비자 플랜에서 제3자 앱의 `agy` 래핑을 지원하지 않고 계정 자동 전환도 금지한다. | ADR-0020의 총괄·카드·예약 실행 및 자동 조회 차단을 유지한다. O는 독립 터미널만 열고 사용자가 직접 명령을 입력한다. |
| Devin | [공식 CLI](https://docs.devin.ai/work-with-devin/devin-cli)는 로컬 실행을 지원하며 설치된 CLI는 `devin acp`를 제공한다. 기존 카드는 `devin -p`와 사용자 본인 로그인을 쓴다. | ACP의 실제 세션 재개·권한 요청·설정 격리를 검증해야 총괄 채팅에 연결할 수 있다. 독립 프로필 미지원과 Windows에서 읽기 전용 보장 범위를 그대로 표시한다. |
| OpenCode | [공식 프로바이더 문서](https://opencode.ai/docs/providers/)의 사용자가 설정한 공급자를 이용한다. | 기존 정책대로 Claude 구독 자격 증명을 O가 넘기지 않는다. 사용량 창이 없어 O의 하루 사용 예산을 적용할 수 없다. |

이 표는 기술·제품 검토이며 개별 사용자의 계약이나 배포 자격을 확정하지 않는다. 프로바이더 문서는 바뀔 수 있으므로 배포 전에 재확인한다.

OpenAI의 [서드파티 app-server 안내](https://developers.openai.com/siwc/token-sharing-open-source/codex-app-server)는 앱에 명시적으로 승인한 OAuth 토큰을 사용하는 경로를 설명한다. 이번 구현은 사용자가 이미 로그인한 로컬 Codex CLI를 사용하므로 그 별도 동의 경로를 구현한 것은 아니다. 로컬 개인 사용과 O의 배포에 같은 인증 방식이 허용되는지는 배포 전에 OpenAI에 확인해야 한다.

Claude 구독을 유지하겠다는 대표 선택에 따라 SDK 연결은 보류한다. Anthropic에 확인할 질문: "사용자 PC에서 O가 Agent SDK로 수정하지 않은 Claude Code를 실행하고, 사용자가 Anthropic 공식 로그인으로 자기 구독에 직접 로그인하는 경우 제품 개발자 인증 조건과 상용 약관상 허용되는가?" 구독 한도에서 차감된다는 사실만으로 제품 개발자의 인증 조건까지 충족한다고 단정할 수 없다.

추가 확인 사항: [제품 내 Claude Code 제공 조건](https://code.claude.com/docs/en/legal-and-compliance)은 상업 약관 동의와 수정 없는 바이너리 사용을 요구하고, 내장 인증 수단을 제거·비활성화·제한하지 못하도록 한다. 현재 O는 원치 않는 API 과금을 피하려고 Claude 총괄과 카드의 자식 프로세스에서 `ANTHROPIC_API_KEY`와 `ANTHROPIC_AUTH_TOKEN` 환경변수를 제거한다. 이 동작이 해당 조건의 인증 수단 제한에 해당하는지도 Anthropic에 확인해야 한다. 이는 기존 O 동작에 대한 미확인 사항이며 위반이라고 확정한 것은 아니다. 확인 전에는 대표가 선택한 구독 사용을 API 과금으로 바꾸지 않는다.

[구독 로그인 안내](https://support.claude.com/en/articles/13189465-log-in-to-your-claude-account)에 따르면 Anthropic은 사용량 크레딧을 켠 일부 유료 구독자의 특정 제3자 도구 사용을 재량으로 허용할 수 있고, 구독 한도 대신 크레딧에서 차감할 권리를 가진다. 별도의 [SDK 안내](https://support.claude.com/en/articles/15036540-use-the-claude-agent-sdk-with-your-claude-plan)는 구독으로 로그인한 SDK 사용은 구독 한도에서 차감된다고 설명한다. 두 안내는 Project O의 허용 여부나 과금 방식을 개별 보증하지 않는다.

Anthropic 문의 초안 (아직 전송하지 않음):

> We are developing a desktop application that starts the unmodified, locally installed Claude Code binary on the user's computer. The user signs in directly through Anthropic's official flow; our application does not collect or pass Claude credentials. We would like to use the Claude Agent SDK with the user's existing Claude subscription, without automatically switching to API billing.
>
> 1. Is this SDK integration permitted for a third-party product and, if so, under which terms and authentication conditions?
> 2. Would the usage count against the subscription allowance or could it be charged to usage credits or an API account? Does the answer depend on the user's plan or product distribution?
> 3. Our existing application removes inherited `ANTHROPIC_API_KEY` and `ANTHROPIC_AUTH_TOKEN` environment variables from the Claude Code child process to avoid unintended API charges. Does this fall within the restriction on disabling or limiting built-in authentication methods? If so, what subscription-preserving approach should we use?

Devin ACP 실동작 검증 시도는 에이전트 보안 훅이 검사할 수 없는 대화형 명령이라고 차단했다. 우회하지 않았으며, ACP 재개 및 권한 검증은 아직 이루어지지 않았다.

### Devin ACP 연결 전 추가 확인

- [Devin 공식 CLI 안내](https://devin.ai/cli)는 읽기 전용 Ask, 세션 재개와 에디터에서의 ACP 사용을 각각 설명한다. 이 설명만으로 O의 ACP 연결에서 Ask 권한이 강제되거나 세션 재개가 동작한다고 보장할 수는 없다.
- 다른 ACP 클라이언트인 [Happier의 자체 통합 문서](https://docs.happier.dev/agents/devin)는 읽기 전용을 Devin의 `ask`에 대응시키고 세션 재개를 지원한다고 설명한다. 같은 문서는 Devin ACP가 표준 세션별 MCP 목록을 무시한다고도 설명한다. 이는 해당 제품의 관찰이지 O에서 확인한 사실이 아니다.
- [OpenAB의 자체 통합 문서](https://github.com/openabdev/openab/blob/main/docs/devin.md)는 자신들의 헤드리스 환경에서 ACP 기본 모드가 파일 수정을 허용하는 `accept-edits`일 수 있다고 설명한다. 버전·환경별 차이를 고려해 O는 기본값이나 환경변수만 믿지 말고, 프롬프트 전 실제 세션이 제공하는 읽기 전용 모드를 설정하고 성공 여부를 확인해야 한다. 설정할 수 없거나 모드를 확인할 수 없으면 총괄을 시작하지 않는다.
- 연결을 구현하려면 별도 검증 환경에서 읽기 전용 모드의 파일 변경·명령 실행 거부, MCP 및 훅 격리, 세션 재개, 중지, 권한 요청과 실패 복구를 실제 설치 버전으로 검증해야 한다. [ACP 세션 설정](https://agentclientprotocol.com/protocol/session-setup)과 [권한 요청](https://agentclientprotocol.com/protocol/tool-calls)은 프로토콜 근거이며 Devin의 구현 보증은 아니다. 기존 보안 훅 차단은 우회하지 않는다.

## pstack

[pstack](https://github.com/cursor/plugins/tree/main/pstack)은 Cursor 플러그인이다. `poteto-mode`가 작업을 조사·버그 수정·기능 구현 등 플레이북으로 분류하고, 역할별 모델과 전문 스킬을 이용해 검증한다. [README](https://github.com/cursor/plugins/blob/main/pstack/README.md), [모드](https://github.com/cursor/plugins/blob/main/pstack/skills/poteto-mode/SKILL.md), [모델 설정](https://github.com/cursor/plugins/blob/main/pstack/docs/guide/01-setup.md).

O에 우선 적용할 수 있는 아이디어:

1. 총괄의 계획 카드에 작업 종류와 완료 증거를 명시한다. 구현·검토·조사를 구분하되, 검증 결과는 실제 실행에서 얻는다.
2. 병렬 카드에는 담당 파일·공유 자원·선행 조건을 적어 충돌을 줄인다. 같은 파일의 동시 수정은 피하고 결과를 한 곳에서 통합한다.
3. 검토 카드는 발견 사항과 재현 근거를 연결한다. 실행하지 않은 검사는 통과로 표시하지 않는다.
4. 모델 역할 설정은 현재 O의 프로바이더·모델 선택과 예산·한도를 먼저 활용한다. pstack의 Cursor 전용 경로와 모델 기본값을 그대로 이식하지 않는다.
5. 도구 실패나 작업 재개 시 완료 조건과 남은 작업을 짧게 복원한다. 기존 O 이벤트·계획 기록으로 표현할 수 있는지 먼저 확인한다.

플러그인 설치나 자동 병합·무인 실행은 이번 조사 결과만으로 수행하지 않는다. 프로젝트 브랜치·승인·프로바이더 정책이 우선한다.
