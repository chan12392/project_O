# ADR-0008: 총괄 채팅 — 읽기 전용 총괄이 계획을 제안하고 사람이 실행한다

- 상태: 승인 (2026-11-02 — 대표 요청으로 구현과 함께 작성)
- 날짜: 2026-11-02
- 승인: [x] A  [ ] B (사후 확인 필요)
- 관련: [GOALS.md](../GOALS.md) 목표 6, ADR-0002(카드 권한), ADR-0004(하루 예산), ADR-0007(모듈 구조), [ARCHITECTURE.md](../ARCHITECTURE.md)

## 배경

앱의 총괄 채팅 입력창은 "보내지 않았습니다"만 표시했다. 계획은 사람이 터미널의 총괄 에이전트(`skills/o`)에게 말하고, 그 에이전트가 `o dispatch`를 부르는 길뿐이었다. 목표 6의 남은 순서 1–3(총괄 세션 설정과 Composer의 실제 전달)을 채우려면 다음을 정해야 했다.

- 총괄이 누구이며 무엇을 할 수 있는가(읽기만 하는가, 실행까지 하는가).
- 대화를 어디에 어떤 형식으로 남기고, CLI 세션을 어떻게 이어 가는가.
- 총괄이 쓴 계획을 어떤 검사를 거쳐 누가 실행하는가.
- 첨부(이미지·문서)를 어디에 두고 무엇을 막는가.

## 선택지

1. 총괄 CLI에 `o dispatch` 실행 권한을 주고 스스로 전달하게 한다 — 사람의 확인 없이 카드가 뜨고, 총괄 CLI에 셸 권한을 열어야 한다.
2. 총괄은 읽기 전용으로 계획만 제안하고, 앱이 그 계획을 같은 정책으로 검사해 보여 준 뒤 사람이 "실행"을 눌러야 O가 전달한다.
3. 총괄 없이 사람이 카드를 직접 작성하는 양식을 둔다 — 카드 나누기·모델 고르기를 사람이 해야 해 목표와 반대다.

## 결정

2를 고른다.

### 대화 스레드 (`crates/o-core/src/lead`)

- 프로젝트마다 현재 스레드 하나(키: 정규화한 프로젝트 경로, Windows는 대소문자 무시). `leads/current.json`이 프로젝트 → 스레드 id, 스레드는 `leads/<threadId>/events.jsonl`. "새 대화"는 새 스레드를 만들고 현재로 바꾼다. 지난 스레드는 지우지 않는다.
- 이벤트(실행의 `events.jsonl`과 따로, 자기 타입 `LeadEvent`): `lead.start`, `lead.user`(글·첨부·에이전트·노력), `lead.session`(CLI 세션 id), `lead.reply`(계획 블록을 뺀 마크다운), `lead.plan`(planId·계획·정책 결과), `lead.dispatch`(planId → runId), `lead.usage`(토큰), `lead.error`. 실행 이벤트 계약(`OEvent`, `src/core/events.ts`)은 바꾸지 않았다.
- 한 스레드에는 한 번에 한 턴만: `running.json`을 `create_new`로 만들고(프로세스가 사라진 표시는 무시), 턴이 끝나면 지운다. "중지"는 `stop` 표시 후 CLI 프로세스 트리를 끝낸다.

### 총괄 실행 (`Provider::lead`, `lead_efforts`)

프로바이더 등록부에 총괄 한 턴의 실행 인자를 더했다. 셸 없이, 창 없이, Job Object로 묶어 프로젝트 폴더에서 띄우고, 설정의 "깨끗한 에이전트"면 카드와 같은 깨끗한 환경을 쓴다. 권한 확인을 끄는 옵션은 없다.

| CLI | 첫 턴 | 이어 말하기 | 읽기 전용 |
|---|---|---|---|
| Claude | `claude -p --output-format stream-json --verbose --permission-mode default --tools Read,Grep,Glob --allowedTools Read,Grep,Glob --strict-mcp-config [--effort] [--add-dir 첨부 폴더]` | `--resume <session>` | 도구가 읽기 셋뿐이고 MCP를 싣지 않는다 |
| Codex | `codex exec -s read-only -C <project> --skip-git-repo-check [-i 이미지]... --json [-c model_reasoning_effort=…] -` | `codex exec … resume [-i …] --json <session> -` | read-only 샌드박스 |
| Antigravity | 제외 | — | agy 1.4는 `--conversation`으로 이어 가지만 읽기 전용이라고 문서화된 모드가 없어 총괄을 맡기지 않는다 |

- 노력 수준은 CLI가 받는 값만: Claude `low·medium·high·xhigh·max`, Codex `minimal·low·medium·high·xhigh`. 모델은 CLI 기본(총괄 모델 지정은 CLI에만 `--model`로 둔다).
- 총괄 CLI는 그 CLI의 API 키 변수(`ANTHROPIC_API_KEY`·`ANTHROPIC_AUTH_TOKEN`, `CODEX_API_KEY`)를 대소문자 구분 없이 물려받지 않는다. 남은 키가 구독 대신 API로 과금되는 것을 막는다(B의 카드 수정과 같은 `api_key_vars`·`names_to_remove`).

### 프롬프트 (`lead::prompt`)

한국어로 짧게. 세션의 첫 턴에만 역할(읽기만, 모호하면 되묻기, 파일이 겹치지 않게 카드 1–5장, 카드마다 provider·model·effort 선택과 이유 한 줄), 상한, 허용 필드, 오늘 예산(`usage::budget`의 한 줄씩), 계획 형식(` ```o-plan ` 블록 정확히 하나, project는 O가 채움)을 보낸다. 이어지는 턴은 이 스레드가 실행한 계획의 상태(카드 상태와 결과 앞 다섯 줄, 2,500자까지)와 사용자 메시지만 보낸다. 같은 스레드에서 에이전트를 바꾸면 새 세션이므로 역할과 최근 대화 여섯 개(각 500자)를 함께 보낸다. 첨부는 저장 경로를 적고, Codex에는 이미지를 `-i`로 넘긴다.

### 계획 검사와 실행

- 답에서 `o-plan` 블록을 읽는다: 없으면 대화만, 하나면 계획, JSON이 아니거나 두 개 이상이면 `lead.error`로 받지 않는다.
- 계획의 `project`는 스레드의 프로젝트로 덮어쓴다. `o dispatch`와 같은 `policy::check_plan`(필드·프로바이더·상한·쿨다운·하루 예산)으로 검사해 결과와 함께 `lead.plan`에 저장한다. 아무것도 실행하지 않는다.
- "실행"(`o lead dispatch <planId> [--clean]`)은 정책을 다시 검사하고, 통과하면 `o dispatch`처럼 실행 폴더를 만들고 `o _run`을 백그라운드로 띄운 뒤 `lead.dispatch`를 남긴다. 한 계획은 한 번만: `dispatched/<planId>`를 `create_new`로 만들어 동시에 눌러도 한 번이다. 차단되면 사유를 그대로 돌려주고 `blocked` 기록에 남긴다.
- 총괄이 쓴 글과 계획은 데이터다. 카드가 되기 전에 사람이 보고, 정책이 두 번 검사한다.

### 첨부 (`lead::files`)

- 이미지(png·jpg·jpeg·gif·webp)와 문서(pdf·md·txt·csv·json·log), 하나에 10MB, 메시지당 5개. 나머지는 이유를 한국어로 말하고 받지 않는다.
- 저장 위치는 O 상태 폴더 밖의 `%LOCALAPPDATA%/project_O-lead/<threadId>/`(총괄 CLI가 읽어야 하므로). 이름은 마지막 경로 조각만 남기고 글자·숫자·한글·`._-` 외는 `_`로, 앞의 점을 떼고, `01-` 같은 번호를 붙인다. 파일은 `create_new`로만 만들어 기존 이름·링크를 따라가지 않고, 링크인 폴더는 쓰지 않는다.
- 로그인 정보 파일 이름(가드 규칙 `CREDENTIAL_RULES`)과 `.claude`·`.codex`·`.gemini`·`.ssh`·`.aws` 아래 파일, 링크·폴더는 첨부하지 않는다.
- 총괄의 프로젝트 폴더도 카드와 같은 규칙(`policy::check_project`)을 따른다: O 상태 폴더·홈 폴더 자체·보호 브랜치 거울은 안 된다.

### 앱

- `o lead send|show|new|attach|dispatch|stop`, Tauri `lead_*` 명령, 개발 서버 `/api/lead/*`, `Backend.lead*`. 첨부는 Tauri에서 원본 바이트(머리글에 프로젝트·이름), 개발 서버에서 요청 본문으로 받아 `o lead attach … -`에 넘긴다. 데모는 "미리보기에서는 총괄에 보내지 않습니다"로 거절한다.
- 입력창: 상자 하나 안에 첨부 칩, 글, 아랫줄 왼쪽 `+`(첨부)와 실행 프로필 칩(깨끗한 에이전트/기본 하네스 — 설정과 같은 값), 오른쪽 에이전트·노력 하나의 선택(설치·로그인된 Claude Code·Codex만), 답하는 동안 돌아가는 표시와 중지(■), 아니면 보내기. Ctrl/⌘+Enter, 이미지 붙여넣기·끌어 놓기.
- 대화: 사용자 글과 첨부, 총괄의 답(마크다운), 계획 카드(카드 id·역할·에이전트·모델·노력·할 일, 정책 결과, 실행 → runId와 그 실행 카드), 오류. 설정 → 총괄에서 기본 에이전트와 노력.
- 데스크톱 앱은 실제 데이터만 보인다: 실행이 없어도 데모로 넘어가지 않고, 첫 실행은 에이전트 연결 → 프로젝트 폴더 → 총괄 대화다. 녹화된 데모는 브라우저 미리보기(`?source=demo`, 개발 서버)에만 있다.
- `o tokens`는 총괄 턴의 토큰을 계획 부대 비용(`lead`)으로 따로 센다.

## 영향

- 총괄은 파일을 고치거나 명령을 실행할 수 없다. 카드 권한(ADR-0002)·정책 상한·예산(ADR-0004)은 바뀌지 않고, 계획은 `o dispatch`와 같은 길로만 실행된다.
- O 상태 폴더에 `leads/`, 그 옆에 `project_O-lead/`가 생긴다. 지워도 실행 기록과 색인에는 영향이 없다.
- 새 프로바이더를 총괄로 쓰려면 `lead_efforts`·`lead`를 구현하고 `crates/o-core/tests/providers.rs`에 정확한 인자와 "쓰기·우회 없음" 사례를 더한다(ARCHITECTURE.md).
- 남은 일: 카드 질문(`o msg` → lead)에 총괄이 답하는 연결, 스트리밍 중간 표시, 총괄 재접속 시 끊긴 턴 회수.
- 되돌리기: 기능 단위 커밋이므로 되돌릴 수 있다. 저장된 스레드는 남지만 읽는 쪽이 없을 뿐이다.
