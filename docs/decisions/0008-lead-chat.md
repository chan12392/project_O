# ADR-0008: 총괄 채팅 — 읽기 전용 총괄이 계획을 제안하고 사람이 실행한다

- 상태: 승인 (2026-11-02 — 대표 요청으로 구현과 함께 작성). 같은 날 개정: 총괄 모델 선택, Antigravity를 총괄에 더함(대표 요청, 아래 "개정 1")
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
| Claude | `claude -p --output-format stream-json --verbose --permission-mode default --tools Read,Grep,Glob --allowedTools Read,Grep,Glob --strict-mcp-config --setting-sources user --settings {"disableAllHooks":true} [--effort] [--add-dir 첨부 폴더]` | `--resume <session>` (같은 인자) | 도구가 읽기 셋뿐이고 MCP를 싣지 않는다. 프로젝트 설정을 읽지 않고 훅을 끈다(개정 2) |
| Codex | `codex exec -s read-only -C <project> --skip-git-repo-check --disable hooks [-i 이미지]... --json [-c model_reasoning_effort=…] -` | `codex exec … --disable hooks resume [-i …] --json <session> -` | read-only 샌드박스, 훅 끔(개정 2) |
| Antigravity | `agy --output-format stream-json --mode plan --sandbox --settings {"permissions":{"deny":["write_file(*)","command(*)","mcp(*)","read_url(*)","execute_url(*)"]}} [--add-dir 첨부 폴더] [--model] [--effort] -p <프롬프트>` | `--conversation <id>` (스트림 `init`의 `conversation_id`) | 세션 설정의 거부 규칙을 agy 권한 검사가 막는다(파일 쓰기·셸·MCP·URL 동작). plan 모드만으로는 막히지 않는다(개정 1). 한 번만 훅을 끄는 공식 옵션이 없어 프로젝트에 `.agents/hooks.json`이 있으면 총괄을 맡지 않는다(개정 2) |

- 노력 수준은 CLI가 받는 값만: Claude `low·medium·high·xhigh·max`, Codex `minimal·low·medium·high·xhigh`, Antigravity `low·medium·high·xhigh·max`. 모델은 기본이 CLI 설정이고, 사람이 고르면 `--model`(Codex `-m`)로 넘긴다(개정 1).
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

- `o lead send|show|new|attach|dispatch|stop|models`, Tauri `lead_*` 명령, 개발 서버 `/api/lead/*`, `Backend.lead*`. 첨부는 Tauri에서 원본 바이트(머리글에 프로젝트·이름), 개발 서버에서 요청 본문으로 받아 `o lead attach … -`에 넘긴다. 데모는 "미리보기에서는 총괄에 보내지 않습니다"로 거절한다.
- 입력창: 상자 하나 안에 첨부 칩, 글, 아랫줄 왼쪽 `+`(첨부)와 실행 프로필 칩(깨끗한 에이전트/기본 하네스 — 설정과 같은 값), 오른쪽 에이전트·모델·노력 하나의 선택(설치·로그인된 총괄 에이전트만 고를 수 있음, 개정 1), 답하는 동안 돌아가는 표시와 중지(■), 아니면 보내기. Ctrl/⌘+Enter, 이미지 붙여넣기·끌어 놓기.
- 대화: 사용자 글과 첨부, 총괄의 답(마크다운), 계획 카드(카드 id·역할·에이전트·모델·노력·할 일, 정책 결과, 실행 → runId와 그 실행 카드), 오류. 설정 → 총괄에서 기본 에이전트와 노력, 에이전트마다 기본 모델.
- 데스크톱 앱은 실제 데이터만 보인다: 실행이 없어도 데모로 넘어가지 않고, 첫 실행은 에이전트 연결 → 프로젝트 폴더 → 총괄 대화다. 녹화된 데모는 브라우저 미리보기(`?source=demo`, 개발 서버)에만 있다.
- `o tokens`는 총괄 턴의 토큰을 계획 부대 비용(`lead`)으로 따로 센다.

## 개정 1 (2026-11-02): 총괄 모델 선택, Antigravity 총괄

대표 요청: 입력창에서 총괄 모델도 고르고, 설치된 agy가 총괄 목록에 보이게 한다.

### 모델

- `o lead send --model <id>`: 카드 `model`과 같은 형식 규칙(`policy::is_model_id`)으로 검사하고 `lead.user`에 남긴 뒤 `Provider::lead`에 넘긴다. 없으면 CLI 기본 모델이다.
- 목록은 각 CLI의 공식 출처만 쓴다(`Provider::lead_models`, `o lead models --provider <id>`). 셸 없이, 창 없이, API 키 변수를 뺀 채 실행한다. 형식 규칙에 맞지 않는 id는 버린다.

| CLI | 출처 |
|---|---|
| Claude Code | 목록 명령이 없어 `claude --help`가 문서화한 별칭 `fable·opus·sonnet·haiku` |
| Codex | `codex debug models`(`codex --help`에 있는 명령)의 모델 카탈로그 중 `visibility: "list"`, `priority` 순 |
| Antigravity | `agy models`의 `id<TAB>이름` 줄 |

- 앱은 목록을 10분 둔다(Tauri `lead_models`, 개발 서버 `/api/lead/models`, 데모는 고정 목록). 목록을 못 받으면 입력창은 "직접 입력"으로 id를 받고, 같은 규칙(`src/core/lead.ts`의 `MODEL_ID`, `tests/lead.test.ts`가 Rust와 비교)으로 검사한다.
- 입력창 선택 메뉴는 에이전트 → 모델 → 노력 수준 순서이고, 첫 모델은 "기본 모델"(`--model` 없음)이다. 버튼은 "Claude Code · Opus · 높음"처럼 보인다. 설정 → 총괄에 에이전트마다 기본 모델을 둔다(이 브라우저의 설정).
- 총괄로 쓸 수 있지만 지금 고를 수 없는 에이전트(설치·로그인 안 됨)도 메뉴에 비활성으로 보이고 이유를 적는다.

### Antigravity

agy 1.4.12 `--help`에 `--mode plan`, `--sandbox`, `--settings`(세션 한정 설정 JSON), `--conversation <ID>`, `--add-dir`, `--model`, `--effort`가 있다. 홈 폴더 밖 임시 프로젝트(`C:\o-smoke-agy`, 파일 하나)에서 `--effort low`로 네 번 확인했다.

| # | 실행 | 결과 |
|---|---|---|
| 1 | `--mode plan --sandbox --settings {"permissions":{"deny":["command(*)","write_file(*)","mcp(*)"]}}`, 파일 고치기와 `cmd /c echo hi > made.txt` 요청 | 모델이 `edit_file`·`run_command`를 시도했고 둘 다 "Matches user-configured deny rule"로 거부. 파일 해시 같음, `made.txt` 없음. agy가 자기 계획 문서(`~/.gemini/.../implementation_plan.md`)를 쓰는 것도 거부됨 |
| 2 | 같은 요청, `--mode plan --sandbox`만(거부 규칙 없음) | 모델이 스스로 고치지 않았지만 자기 계획 문서는 썼다. 1에서 plan 모드인데도 `edit_file`을 시도했으므로 plan 모드만으로는 읽기 전용을 보장하지 못한다 |
| 3 | 1의 대화를 `--conversation <id>`로 이어 감, 최종 거부 규칙(+`read_url(*)`·`execute_url(*)`) | `init`과 결과의 `conversation_id`가 같고 앞 턴 내용을 기억함. 다시 시도한 셸 명령은 거부. 파일 그대로 |
| 4 | 같은 명령을 `allow`에도 넣고 `deny`와 함께 | 모델이 시도하지 않아 allow·deny 우선순위는 확인하지 못함. 파일 그대로 |

- 결정: 읽기 전용은 plan 모드가 아니라 거부 규칙이 보장하므로, agy 총괄은 거부 규칙을 항상 넣는다. 허용(`allow`) 규칙은 넣지 않는다. 권한 확인을 끄는 옵션이나 `accept-edits`·`auto` 모드는 쓰지 않는다(`crates/o-core/tests/providers.rs`가 고정).
- agy는 이미지 입력 옵션이 없다. 첨부는 프롬프트에 경로를 적고 첨부 폴더를 `--add-dir`로 연다(읽기만).
- 남은 위험: 사람의 agy 사용자 설정에 넓은 `allow` 규칙이 있을 때 세션 `deny`가 이기는지는 직접 확인하지 못했다(4번). 총괄의 계획은 그대로 사람이 "실행"을 눌러야 카드가 되고 정책을 두 번 거친다.

## 개정 2 (2026-11-02): 정기 코드 검토·UX 감사 반영

### 프로젝트 훅

- `claude -p`는 폴더 신뢰 대화상자를 띄우지 않지만 프로젝트 설정 파일의 훅은 실행한다(code.claude.com/docs/en/permissions#what-runs-before-you-trust-a-folder). 그래서 총괄은 첫 턴·이어 말하기 모두 `--setting-sources user`(깨끗한 프로필이면 그 프로필 설정이 user)와 `--settings {"disableAllHooks":true}`로 띄운다. 공식 문서가 권하는 두 옵션이다.
- Codex는 신뢰한 프로젝트의 `.codex/`에서 훅을 싣는다. `--disable hooks`(`codex exec --help`: `-c features.hooks=false`와 같음, 모든 설정 층보다 우선)로 끈다.
- agy는 프로젝트 `.agents/hooks.json`을 싣고(antigravity.google/docs/hooks) 한 번만 훅을 끄는 공식 옵션이 없다. 그 파일이 있는 프로젝트에서는 agy 총괄을 거절하고 Claude Code·Codex를 고르게 한다.

### 첨부 경로

- 경로로 받은 파일은 열기 전에 위쪽 폴더를 하나씩 확인해 링크(심볼릭 링크·정션)가 하나라도 있으면 거절하고, 실제 경로(`canonicalize`)를 다시 로그인 정보 폴더·파일 이름 규칙으로 검사한 뒤 그 실제 경로를 연다. `.claude`를 가리키는 별칭 폴더 아래의 평범한 파일이 통과하던 문제를 막는다.

## 영향

- 총괄은 파일을 고치거나 명령을 실행할 수 없다. 카드 권한(ADR-0002)·정책 상한·예산(ADR-0004)은 바뀌지 않고, 계획은 `o dispatch`와 같은 길로만 실행된다.
- O 상태 폴더에 `leads/`, 그 옆에 `project_O-lead/`가 생긴다. 지워도 실행 기록과 색인에는 영향이 없다.
- 새 프로바이더를 총괄로 쓰려면 `lead_efforts`·`lead`를 구현하고 `crates/o-core/tests/providers.rs`에 정확한 인자와 "쓰기·우회 없음" 사례를 더한다(ARCHITECTURE.md).
- 남은 일: 카드 질문(`o msg` → lead)에 총괄이 답하는 연결, 스트리밍 중간 표시, 총괄 재접속 시 끊긴 턴 회수.
- 되돌리기: 기능 단위 커밋이므로 되돌릴 수 있다. 저장된 스레드는 남지만 읽는 쪽이 없을 뿐이다.
