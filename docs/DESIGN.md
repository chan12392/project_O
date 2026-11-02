# 설계 초안 (v0)

> 상태: 초안. 아래 "결정 필요" 항목을 두 사람이 합의한 뒤 코드 뼈대를 만든다. 합의된 결정은 `docs/decisions/`에 ADR로 옮긴다.

## 0. 한 줄 정의

사용자 개인이 자기 PC에서 자기 구독 계정으로 쓰는 로컬 앱. 비유하면 **Orca의 채팅 UI 버전**이다. 서버를 두지 않고, 다른 사람의 계정을 대신 쓰게 하지 않는다.

## 1. 풀고 싶은 문제

| | A — 관제탑 | B — 통합 작업대 |
|---|---|---|
| 한 줄 | 흩어진 에이전트 세션을 한 화면에서 관측하고, 사람이 개입할 일을 놓치지 않는다 | Claude·Codex 등 여러 구독 계정과 모델을 한 데스크톱 앱에서 쓰고, 역할별로 라우팅한다 |
| 답하는 질문 | 지금 뭐가 돌고 있나? 누가 나를 기다리나? 어디까지 했고 뭘 하다 말았나? | 이 일은 어느 모델·계정으로 시킬까? 계정을 바꾸지 않고 바로 이어서 쓸 수 있나? |
| 출발점 | 기존 Orchestrator 앱 (Tauri 2 + React + TS + SQLite, Local/Orca 세션 관측, PR 정책) | 터미널이 아닌 데스크톱 형태, 비개발자도 쓰기 쉬운 UX |
| 주된 동작 | 읽기(관측) + 제한된 쓰기(PR 머지 등) | 쓰기(에이전트 실행·대화) |

## 2. 두 방향의 관계

두 방향은 같은 제품의 다른 층이다. B의 작업대에서 시작한 세션이 그대로 A의 관제탑에 보이면, 하나의 앱이 "시키고 → 지켜보고 → 판단한다"를 모두 덮는다.

```
┌──────────────── 표면(Surface) ────────────────┐
│  Workspace (B)            Control (A)          │
│  대화·실행·계정 선택       Dashboard·Board·PR   │
└───────────────┬───────────────────┬───────────┘
                │                   │
┌───────────────▼───────────────────▼───────────┐
│  Core (공용, React를 모름)                     │
│  Session 모델 · Router · Policy · Store        │
└───────┬───────────────────────────┬───────────┘
        │                           │
┌───────▼────────┐        ┌─────────▼──────────┐
│ Providers (B)  │        │ Sources (A)        │
│ Claude Code    │        │ Local 세션 인덱스  │
│ Codex          │        │ Orca               │
│ Gemini ...     │        │ git / GitHub       │
│ + Account vault│        │                    │
└────────────────┘        └────────────────────┘
```

- **Providers**: 에이전트를 실행하고 대화를 주고받는 쪽(쓰기). 계정별 설정 폴더를 격리해 여러 구독 계정을 동시에 쓴다.
- **Sources**: 이미 돌고 있는 세션을 관측하는 쪽(읽기). Orchestrator의 기존 코드가 여기에 해당한다.
- 앱이 직접 실행한 세션은 Provider가 Session 모델로 기록하므로, Control 화면은 외부 세션과 앱 내부 세션을 같은 방식으로 보여 준다.

## 3. 원칙 (초안)

1. 에이전트의 자기보고는 힌트, git/GitHub이 진실. (Orchestrator 원칙 계승)
2. 라우팅은 대화 한 턴이 아니라 **작업(세션) 단위**로 한다. 모델을 바꿀 때마다 기본 지침·하네스가 다시 주입되어 토큰이 늘어나는 문제를 피하기 위해서다. 모델 간 인계는 전체 대화가 아니라 짧은 인계서(요약·결정·남은 일)로 한다.
3. 계정 인증은 각 공식 CLI의 로그인을 그대로 쓴다. 앱은 인증 파일을 읽거나 복사하거나, 그 토큰으로 직접 요청하지 않는다. (Claude·Gemini 약관이 금지하는 지점이 여기다 — §8)
4. 채팅 화면은 터미널(TUI) 출력을 긁어 다시 그리지 않는다. 공식 CLI의 구조화 출력(JSON 스트림·app-server)을 받아 그린다.
5. `core`는 UI를 모르고, 화면은 core가 만든 스냅샷을 그릴 뿐 판단하지 않는다.

## 4. 결정 필요 (코드 뼈대 전에 합의)

| ID | 질문 | 선택지 | 초안 제안 |
|---|---|---|---|
| D1 | 기술 스택 | (a) Orchestrator와 같은 Tauri 2 + React + TS (b) Electron (c) 기타 | (a) — 기존 관측 코드를 옮겨 쓸 수 있고 설치 파일이 가볍다 |
| D2 | Orchestrator 코드 이관 범위 | (a) `src/core` 순수 로직만 가져오기 (b) 앱 전체를 포크 (c) 새로 작성 | (a) — 화면은 새 정보구조에 맞춰 다시 짠다 |
| D3 | 에이전트 연결 방식 | TUI 스크래핑 / 터미널 임베드 / 공식 CLI 구조화 출력 / API 키 직접 호출 | 공식 CLI 구조화 출력 — Claude `claude -p --input-format stream-json --output-format stream-json`, Codex `codex app-server`, Gemini `gemini -p --output-format stream-json`. 세부는 PoC로 확인 (§5) |
| D4 | 여러 계정 격리 방식 | 계정별 설정 폴더 환경변수 / OS 사용자 분리 / 기타 | 설정 폴더 격리 PoC 먼저 |
| D5 | 라우팅 | 수동 선택 / 역할 프리셋(설계·조사·정리) / 자동 분류 | 총괄이 카드마다 에이전트·모델·노력 수준을 일에 맞게 정한다(자동 분배). 사용자가 특정 에이전트·모델을 말하면 그대로 따른다. 역할(`role`)은 선택 이름표일 뿐 모델을 고르거나 제한하지 않는다(2026-11-02 대표님 결정·정정) |
| D6 | 첫 화면·최상위 구조 | Workspace 우선 / Control 우선 / 두 모드 전환 | 두 모드 전환, 첫 화면은 합의 |
| D7 | MVP 범위 | 아래 §6 | 합의 |
| D8 | 총괄 에이전트 형태 | (a) 앱 코드의 규칙 라우터 (b) 공식 CLI 세션이 O 스킬로 `o dispatch` 호출 (c) API 키 모델 | (b)로 구현 시작 (`src/cli/o.ts`, `skills/o`). 앱이 구독 토큰으로 직접 계획 요청을 보내는 방식은 금지 (§8) |

## 5. 검증할 것 (PoC)

결정 전에 사실 확인이 필요한 항목. 각자 작업 브랜치에서 작게 확인하고 결과를 이 표에 채운다.

| 항목 | 확인할 내용 | 담당 | 결과 |
|---|---|---|---|
| Claude Code 연결 | 비대화 실행·스트리밍 출력·세션 재개가 앱에서 다루기 충분한가 | | |
| Codex 연결 | 같은 내용 + 계정 2개를 동시에 쓸 수 있는가 | | |
| 계정 격리 | 계정별 설정 폴더로 로그인 상태가 섞이지 않는가 | | |
| 승인 처리 | 파일 수정·명령 실행 승인을 채팅 화면 버튼으로 받을 수 있는가 (Claude 권한 프롬프트 도구, Codex app-server 승인 요청, Gemini 헤드리스 승인 모드) | | |
| 세션 재개·중단 | 앱을 껐다 켜도 같은 대화를 이어 가고, 실행 중 작업을 멈출 수 있는가 | | |
| 라우팅 토큰 비용 | 작업 단위 전환 시 지침 재주입 토큰이 얼마인가 | | |
| 약관 | 각 서비스 약관상 본인 구독 계정을 이런 방식으로 써도 되는가 | | |

## 6. MVP 후보

1. 계정 등록: Claude 1개, Codex 2개를 앱에 등록(각 CLI 로그인 이용).
2. Workspace: 프로젝트 폴더를 고르고, 역할 프리셋으로 모델·계정을 골라 세션을 시작·대화한다.
3. Control: 앱에서 시작한 세션과 외부 세션(Local/Orca)이 한 Board에 보이고, 나를 기다리는 세션이 맨 위에 온다.

## 7. 영역 분담 (초안)

| 영역 | 주 담당 | 비고 |
|---|---|---|
| Providers · Account vault · Router · Workspace 화면 | B | |
| Sources · Policy · Control 화면 | A | Orchestrator 코드 이관 |
| Core Session 모델 · Store · 앱 셸 | 공동 | 변경 시 ADR + 상호 승인 |

## 8. 프로바이더 정책 (2026-11-01 확인, 배포 전 재확인)

| | 하면 안 되는 것 | 이 앱이 쓰는 경로 |
|---|---|---|
| Claude | 앱에 Claude.ai 로그인을 넣기, 구독 토큰으로 대신 요청하기, 인증 정보 수집·저장 | 수정하지 않은 Claude Code를 본인 로그인으로 실행. 공식 도움말: Claude 플랜으로 로그인한 Agent SDK·`claude -p` 사용량은 구독 한도에서 차감 |
| Gemini | Gemini CLI의 OAuth로 외부 도구가 백엔드에 접속 (계정 정지 사유) | 공식 `gemini` CLI 헤드리스 실행. 구독 플랜이 CLI에서 되는지는 미확인 → 안 되면 AI Studio API 키 |
| Codex | 유료·원격 호스팅 앱에서 구독 사용 (별도 신청 필요) | 로컬 `codex app-server` (공식 클라이언트 프로토콜) |

- 구독 한도는 일반적인 개인 사용을 전제로 한다. 계정을 돌려 가며 한도를 피하는 용도로 쓰지 않는다.
- 카드 CLI를 띄울 때 API 키 환경변수(Claude `ANTHROPIC_API_KEY`·`ANTHROPIC_AUTH_TOKEN`, Codex `CODEX_API_KEY`)는 물려주지 않는다. 헤드리스 실행에서는 이 변수가 각 CLI의 로그인보다 먼저 쓰여서, PC에 남은 키가 있으면 구독 대신 API로 과금된다([Claude Code 인증 우선순위](https://code.claude.com/docs/en/authentication), Codex `codex-rs/login/src/auth/manager.rs`). 구현: `crates/o-core/src/providers.rs`의 `api_key_vars`.
- 총괄 에이전트가 작업을 나눠 다른 CLI에 넘기는 것은 허용 범위다. 각 단계가 공식 CLI 자신의 로그인으로 실행되기 때문이다. 단, 사람이 보지 않는 대량·상시 자동 실행은 하지 않는다 — 실행 동시성과 하루 작업 수에 상한을 둔다. Anthropic 소비자 약관은 API 키나 명시 허용 외의 자동화 접근을 금지하며, `claude -p`는 공식 도움말이 허용한 경로다.
- 출처: [Claude Code 법률·준수](https://code.claude.com/docs/en/legal-and-compliance), [Claude 플랜으로 Agent SDK 사용](https://support.claude.com/en/articles/15036540), [Gemini CLI FAQ](https://geminicli.com/docs/resources/faq/), [Gemini CLI 헤드리스](https://www.geminicli.com/docs/cli/headless), [OpenAI 토큰 공유](https://developers.openai.com/siwc/token-sharing-open-source)
- 자동 차단 구현: `crates/o-guard`(에이전트 훅·git 훅·CI), `crates/o-core/src/policy.rs`(O CLI). 목록은 AGENTS.md "프로바이더 정책 자동 차단".

## 9. 화면 계약 (다음 단계)

채팅 UI와 캐릭터 화면은 O CLI가 쓰는 `events.jsonl`(Rust `crates/o-core`가 쓰고, 화면은 `src/core/events.ts`의 `OEvent` 타입으로 읽음)만 읽는다. 프로바이더별 출력 형식은 화면에 오지 않는다.

| 이벤트 | 채팅 UI | 오른쪽 캐릭터 화면 |
|---|---|---|
| `run.start` | 작업 묶음 헤더 (제목·카드 목록) | 카드 수만큼 캐릭터 배치 (프로바이더별 캐릭터) |
| `card.queued` | "대기" 표시와 사유 | 대기 자세 |
| `card.start` / `card.tool` | 카드 진행 줄 (도구·파일) | 작업 중 애니메이션, 말풍선에 도구 이름 |
| `card.message` | 에이전트 메시지 | 말풍선 |
| `card.limit` | 한도 경고 (차단 시각) | 휴식 자세 |
| `card.end` | 결과 요약, 실패·중지 이유 | 완료·실패 표시 |

### 에이전트 작업 탭 (2026-11-02)

Orchestrator 앱의 대시보드와 Next up을 O 프로젝트 하나에 맞춰 가져왔다. 판단 규칙은 `src/core/attention.ts`(순수 함수, 테스트 `tests/attention.test.ts`)에 있고 화면은 그리기만 한다.

| 하위 보기 | 내용 |
|---|---|
| 대시보드 | 기다리는 일(답하기·실패·응답 없음·한도·PR 검토, 오래 기다린 것부터) → 현황 줄(작업 중·멈춤·PR·오늘 카드) → 작업 흐름(카드별 실행 구간, 10분 넘는 빈 시간은 접음) → 활동 |
| 다음 할 일 | **다음 할 일**: 총괄에게 온 메시지 중 총괄이 아직 답하지 않은 것, 에이전트가 보고한 PR, 24시간 안에 끝난 실행의 결과 확인, 곧 시작할 카드 / **멈춘 작업**: 실패, 응답 없음(실행 중인데 10분째 기록 없음), 한도, 중지 |
| 보드 | 카드 칸반 (대기·작업 중·PR·완료·막힘) |

- 티켓은 Orchestrator의 감열지 표현을 따른다. 윗변이 기다린 시간만큼 그을리고, 도장이 기다림의 종류를 말한다.
- 처리함 체크는 이 브라우저에만 남는다(`project-o-handled`). 쓰기 동작은 없다. 답장·중지는 `o msg send`·`o stop` 명령을 복사해 터미널에서 실행한다.
