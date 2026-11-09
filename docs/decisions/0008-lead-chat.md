# ADR-0008: 총괄 채팅 — 읽기 전용 총괄이 계획을 제안하고 사람이 실행한다

- 상태: 승인 (2026-11-02 — 대표 요청으로 구현과 함께 작성). 같은 날 개정: 총괄 모델 선택, Antigravity를 총괄에 더함(대표 요청, 아래 "개정 1"). 2026-11-04 개정 3: 일반 모드의 건별 승인과 진행 단계(대표 요청)
- 날짜: 2026-11-02
- 승인: [x] A  [ ] B (사후 확인 필요)
- 관련: [GOALS.md](../GOALS.md) 목표 6, ADR-0002(카드 권한), ADR-0004(하루 예산), ADR-0007(모듈 구조), [ARCHITECTURE.md](../ARCHITECTURE.md)

## 개정 4 (2026-11-08): 여러 총괄 탭

[ADR-0034](0034-lead-tabs.md)가 아래의 "프로젝트마다 현재 스레드 하나"를 바꾼다. 프로젝트마다 열린 대화는 최대 4개이고, `leads/open.json`은 열린 대화와 직접 수정을 맡는 writer 하나를, `current.json`은 마지막으로 고른 탭을 기록한다. 새 대화는 기존 탭을 유지하며, writer 외의 탭과 전체 총괄은 읽기 전용이다. 한 대화의 턴 잠금은 유지하되 서로 다른 열린 탭은 동시에 답할 수 있다. 대화를 지정한 중지는 그 탭만, 프로젝트 중지는 그 프로젝트의 열린 탭 전부, 전체 중지는 모든 열린 탭을 대상으로 한다. 이전 상태에 열린 목록이 없으면 현재 대화 하나를 사용한다.

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
- 한 스레드에는 한 번에 한 턴만: 턴은 `turn.lock`에 OS 배타 잠금을 잡고(개정 2), 표시 `running.json`은 통째로(이름 바꾸기로) 쓰며, 턴이 끝나면 지운다. "중지"는 그 턴의 토큰을 적은 `stop` 요청이고, 턴의 주인이 CLI를 띄우기 전이면 띄우지 않고 띄운 뒤면 그 트리를 끝낸다(개정 2).

### 총괄 실행 (`Provider::lead`, `lead_efforts`)

프로바이더 등록부에 총괄 한 턴의 실행 인자를 더했다. 셸 없이, 창 없이, Job Object로 묶어 프로젝트 폴더에서 띄우고, 설정의 "깨끗한 에이전트"면 카드와 같은 깨끗한 환경을 쓴다. 권한 확인을 끄는 옵션은 없다.

| CLI | 첫 턴 | 이어 말하기 | 읽기 전용 |
|---|---|---|---|
| Claude | `claude -p --output-format stream-json --verbose --permission-mode default --tools Read,Grep,Glob --allowedTools Read,Grep,Glob --strict-mcp-config --setting-sources user --settings {"disableAllHooks":true} [--effort] [--add-dir 첨부 폴더]` | `--resume <session>` (같은 인자) | 도구가 읽기 셋뿐이고 MCP를 싣지 않는다. 프로젝트 설정을 읽지 않고 훅을 끈다(개정 2) |
| Codex | `codex exec -s read-only -C <project> --skip-git-repo-check --disable hooks [-i 이미지]... --json [-c model_reasoning_effort=…] -` | `codex exec … --disable hooks resume [-i …] --json <session> -` | read-only 샌드박스, 훅 끔(개정 2) |
| Antigravity | `agy --output-format stream-json --mode plan --sandbox --settings {"permissions":{"deny":["write_file(*)","command(*)","mcp(*)","read_url(*)","execute_url(*)"]}} [--add-dir 첨부 폴더] [--model] [--effort] -p <프롬프트>` | `--conversation <id>` (스트림 `init`의 `conversation_id`) | 세션 설정의 거부 규칙을 agy 권한 검사가 막는다(파일 쓰기·셸·MCP·URL 동작). plan 모드만으로는 막히지 않는다(개정 1). 한 번만 훅을 끄는 공식 옵션이 없어 프로젝트에 `.agents/hooks.json`이 있으면 총괄을 맡지 않는다(개정 2) |

- 노력 수준은 CLI가 받는 값만: Claude `low·medium·high·xhigh·max`, Codex `minimal·low·medium·high·xhigh`, Antigravity `low·medium·high·xhigh·max`. 모델은 기본이 CLI 설정이고, 사람이 고르면 `--model`(Codex `-m`)로 넘긴다(개정 1).
- 인증 환경변수 전달과 API 과금 경고는 [ADR-0035](0035-api-key-env-pass-through.md)가 이 규칙을 대체한다.

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
- 목록은 각 CLI의 공식 출처만 쓴다(`Provider::lead_models`, `o lead models --provider <id>`). 셸 없이, 창 없이 실행한다. 인증 환경은 ADR-0035를 따른다. 형식 규칙에 맞지 않는 id는 버린다.

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

### 한 턴 잠금

- 턴이 도는지는 표시 파일이 아니라 `turn.lock`의 OS 배타 잠금(`File::try_lock`)으로 판단한다. 잠금은 그 프로세스가 어떻게 끝나든 OS가 풀어 주므로 앱이 답하는 도중 꺼져도 "답하는 중"에 갇히지 않고, pid가 다른 프로세스에 재사용돼도 상관없다.
- 잠금을 잡은 채 표시가 비었거나 덜 쓰였으면 "시작하는 중"(`busy: "starting"`)이다. 다른 보내기가 그 표시를 지우고 두 번째 턴을 띄우던 경쟁을 막는다. 표시는 임시 파일에 쓰고 이름을 바꿔 통째로만 보인다.

### 대화 목록

- `leads/current.json`은 임시 파일에 쓰고 이름을 바꿔 통째로 바꾸며, 바꾸는 동안 `leads/current.lock`에 OS 잠금을 잡는다. 읽지 못한 목록을 빈 목록으로 보지 않는다(짧게 다시 읽고, 그래도 안 되면 오류). 2초 조회와 다른 프로젝트의 새 대화가 겹쳐 대화가 저절로 새로 시작되던 문제(감사 BK9)와, 동시에 보낸 첫 메시지가 스레드를 둘 만들던 경쟁을 막는다.

### 세션 색인

- `o runs --sessions`(2초마다)가 총괄의 CLI 세션을 빼려고 매번 모든 스레드의 이벤트를 읽던 것을, 스레드마다 `sessions.txt`(세션 id 한 줄씩)만 읽게 바꿨다(감사 BK14의 총괄 쪽). 색인이 없는 옛 스레드는 한 번 이벤트에서 만든다. 실행 이벤트 전체를 IPC로 보내는 나머지 비용은 `sessions.rs`·`useLiveRuns.ts` 쪽 일이다.

### 중지

- 표시는 주인(앱이나 `o lead send`의 pid)과 CLI(자식 pid와 시작 시각)를 따로 적는다. "중지"는 아무것도 직접 죽이지 않고 그 턴의 토큰을 `stop`에 적는다. 주인은 CLI를 띄우기 직전에 확인해 띄우지 않고, 띄운 뒤에는 감시 스레드가 0.1초마다 보고 자기 자식 트리를 끝낸다. 자식 핸들을 쥔 채라 pid 재사용을 걱정할 필요가 없고, 별도 `o lead stop`이 앱 자체를 끝내는 일도 없다.
- 이전 턴의 `stop`은 슬롯을 잡을 때 지워 다음 턴을 멈추지 않는다. 시작 직후 몇 초 동안 중지가 무시되던 문제(감사 BK2)도 같은 원인이다.
- 한 턴은 15분(`lead::TURN_LIMIT`)까지다. 넘으면 감시 스레드가 CLI 트리를 끝내고 "제한 시간" 오류를 남긴다. 데스크톱 `lead_send`에 시간 제한이 없던 문제(감사 BK8)를 o-core에서 한 번에 막는다.
- 주인이 죽고 CLI만 남은 경우(kill-on-close 작업 객체가 없을 때)는 잠금을 잡은 채 pid와 시작 시각이 둘 다 맞을 때만 끝내고 표시를 지운다.

### 에이전트를 오가며 이어 말하기

- CLI가 한 턴을 받으면 `lead.seen`(에이전트·세션 id·그 턴의 `lead.user` 위치)을 남긴다. 같은 세션을 이어 쓸 때 그 위치 다음 턴부터의 대화(다른 에이전트가 총괄로 나눈 것, 최근 12개·각 500자)를 프롬프트에 넣는다. Claude → Codex → Claude에서 Codex에게 준 제약을 돌아온 Claude가 모르던 문제를 막는다. 이 이벤트가 없던 스레드는 그 에이전트에게 보낸 마지막 메시지를 기준으로 삼는다.

### 첨부 경로

- 경로로 받은 파일은 열기 전에 위쪽 폴더를 하나씩 확인해 링크(심볼릭 링크·정션)가 하나라도 있으면 거절하고, 실제 경로(`canonicalize`)를 다시 로그인 정보 폴더·파일 이름 규칙으로 검사한 뒤 그 실제 경로를 연다. `.claude`를 가리키는 별칭 폴더 아래의 평범한 파일이 통과하던 문제를 막는다.

## 개정 3 (2026-11-04): 일반 모드 — 읽기는 자동, 나머지는 건별 승인, 진행 단계 기록

대표 요청: 일반 모드(YOLO 꺼짐) 총괄도 파일을 고치거나 명령을 실행할 수 있게 하되 사람이 하나씩 허용한 것만 실행한다. 승인은 화면 전체를 덮는 창이 아니라 입력창 바로 위의 카드로 묻는다. 진행 표시는 도구별로 자세히 보이고, 답이 온 뒤에도 답 위에 접힌 "작업 n단계"로 남는다. YOLO 모드는 바꾸지 않는다.

### 무엇이 자동이고 무엇을 묻나

- 읽기·검색(Claude `Read`·`Grep`·`Glob`, Codex 읽기 전용 샌드박스 안의 읽기)은 묻지 않는다.
- 파일 쓰기·수정, 셸 명령, 웹 검색·가져오기, 그 밖의 도구는 한 번에 하나씩 사람의 허용을 받아야 실행된다. "항상 허용"이나 세션 단위 허용은 없다. 거부가 기본(카드가 뜨면 포커스가 거부에 있음)이다.
- 요청마다 10분(`approvals::WAIT`)을 기다리고, 지나면 거부(`timeout`)한다. 기다리는 동안 턴의 15분 제한(`TURN_LIMIT`)은 흐르지 않는다(감시 스레드가 `approvals/`에 요청이 있는 시간만큼 마감을 미룬다).
- "중지"는 기다리는 요청을 `stopped`로 거부하고(Claude는 `interrupt`, Codex는 `cancel`) 그 턴을 끝낸다. 턴이 끝날 때 남은 요청은 닫는다.
- 예약 실행과 카드 질문에 답하는 턴은 물어볼 사람이 없으므로 이전처럼 읽기 전용이다. O CLI(`o`)를 찾지 못하거나 Codex에 app-server가 없으면 경고를 남기고 그 턴은 읽기 전용으로 돈다.

### Claude Code

공식 `--permission-prompt-tool`로 O의 stdio MCP 서버에 묻는다. 홈 폴더 밖 임시 폴더에서 Claude Code 2.1.302로 확인했다(허용 → 파일 생성, 거부 → `tool_result` 오류와 재시도 없는 설명).

| 인자 | 값 |
|---|---|
| 도구 | `--tools Read,Grep,Glob,Edit,Write,Bash[,PowerShell(Windows)],WebSearch,WebFetch` |
| 묻지 않는 도구 | `--allowedTools Read,Grep,Glob` (이전과 같음) |
| 막는 명령 | `--disallowedTools Bash(o dispatch*)…`와 Windows의 같은 `PowerShell(…)` 규칙 |
| 승인 | `--strict-mcp-config --mcp-config {"mcpServers":{"o_lead":{"command":"<o>","args":["_lead-approve",<home>,<thread>,<token>,<project>]}}} --permission-prompt-tool mcp__o_lead__decide` |
| 설정 | `--setting-sources user --settings {"disableAllHooks":true,"permissions":{"ask":["Bash","PowerShell","Edit","Write","NotebookEdit","WebFetch","WebSearch"]}}` — 사람의 사용자 설정에 있는 allow 규칙이 묻지 않고 통과시키지 않게 |

- `o _lead-approve`는 O 실행 파일 자신(데스크톱 앱은 옆의 `o.exe`)이고, MCP 순서(`server/discover` 탐색에 빈 결과, `initialize`, `tools/list`, `tools/call`)에 답한다. `tools/call`마다 따로 기다리므로 동시에 온 요청은 차례로 카드에 뜬다. 오래 기다리면 `_meta.progressToken`으로 진행 알림을 보낸다.
- 허용은 `{"behavior":"allow","updatedInput":<원래 입력>}`(입력을 바꾸지 않음), 거부는 `{"behavior":"deny","message":…}`.

### Codex (ADR-0023 개정)

app-server 일반 모드는 샌드박스를 그대로 읽기 전용(`sandbox: read-only`, `sandboxPolicy: readOnly`)으로 두고 승인 정책만 `on-request`(검토자 `user`)로 바꾼다. 쓰기 샌드박스(`workspace-write`)는 쓰기가 승인 없이 통과하므로 쓰지 않고, 폐기 예정인 `untrusted`도 쓰지 않는다.

- `item/commandExecution/requestApproval` → 명령 전체·실행 위치·사유를 카드에 보이고 `accept`·`decline`·`cancel`(중지) 중 하나로만 답한다. `acceptForSession`이나 정책 추가는 보내지 않는다. 승인된 권한 상승 명령은 샌드박스 밖에서 돌기 때문에 명령을 줄이지 않고 그대로 보인다.
- `item/fileChange/requestApproval` → 요청에는 diff가 없어 같은 item의 `item/started`·`item/fileChange/patchUpdated`의 `changes[]`(경로·종류·diff)로 파일별 `+N −M`과 diff를 보인다.
- 권한·MCP 입력·사용자 질문 요청은 이전처럼 거절한다.

### O가 먼저 막는 것

사람에게 묻기 전에 O의 가드 규칙(`o_guard::hook::review` — 로그인 정보 파일, 프로바이더 직접 호출, 권한 확인을 끄는 옵션, 보호 브랜치 커밋·push, 거울 폴더)과 O 상태 폴더를 가리키는 쓰기·명령, MCP 도구 호출을 거부한다(`by: guard`). 사람에게는 묻지 않고 기록만 남긴다.

### 자기 승인 차단과 결정 경로

- 결정은 앱의 허용/거부 버튼(Tauri `lead_decide`, 개발 서버 `POST /api/lead/decide`), 또는 사람이 직접 친 `o lead decide <id> allow|deny`뿐이다. 에이전트 환경(`O_AGENT_ROLE`·`O_CARD_ID`)에서는 `o lead decide`를 거절하고, 가드 훅도 같은 명령을 막으며, 총괄의 셸에는 `o lead*` 거부 규칙이 있다.
- 결정 파일은 임시 파일을 `<id>.decision`으로 연결(hard link)해 한 번만 생긴다. 지금 도는 턴(잠금과 표시의 token이 같은 요청)만 결정을 받는다.
- 총괄의 글이나 첨부, 카드의 메시지에 적힌 "승인"은 결정이 아니다. 카드에 총괄의 요청 원문을 그대로 보이고 "채팅의 '승인'은 결정이 아님"을 적는다.

### 기록

| 파일·이벤트 | 내용 |
|---|---|
| `leads/<thread>/approvals/<id>.json` | 기다리는 요청(턴 token, 종류, 요약, 원문 16,000자까지, 파일별 `+/−`·diff, 만료 시각). `lead show`의 `thread.approvals`(token 제외) |
| `lead.approval` | `{at,id,provider,kind:command·file·web·other,tool,summary,detail,cwd,reason,files[{path,added,removed,diff}],expires}`. id는 `<threadId>-a<n>` |
| `lead.decision` | `{at,id,decision:allow·deny·timeout·stopped,by:user·guard·timeout·stop·o,reason}` |
| `lead.step` | 끝난 단계 `{at,id,kind:read·search·run·edit·web·other,label,file,added,removed,status:done·failed·denied·incomplete}`. 턴당 200개까지. 진행 중 단계는 `thread.steps`(임시) |

- `+N −M`은 실제 값이다. Claude는 실행 뒤 `tool_use_result.structuredPatch`(새 파일은 내용 줄 수, 없으면 바꾼 문자열), Codex는 `fileChange.changes[].diff`에서 센다.
- 화면: 진행 중에는 "읽는 중 a.ts", "실행 중 npm test", "'README.md' +12 −3", "웹 검색 중 …"을 실시간으로, 끝나면 그 턴의 답 위에 접힌 "작업 n단계 · 파일 k개 +N −M"으로 남긴다. 승인 카드는 입력창 바로 위에 하나씩(남은 개수 표시) 뜨고 남은 시간을 센다.

### 남은 위험

- Codex의 읽기 전용 총괄에는 Claude의 경로별 Read 거부 규칙에 해당하는 장치가 없어 CLI 로그인 정보 파일의 자동 읽기를 차단하지 못한다.
- "내 설정"(평소 프로필)으로 돌 때 사람의 플러그인·관리 정책이 O보다 먼저 허용할 수 있다. 실행 설정의 `permissions.ask`가 사용자 allow 규칙보다 우선하지만 관리 정책이나 다른 경로는 확인하지 못했다. 카드에 이 한계를 적는다.
- 허용한 명령이 무엇을 하는지는 사람이 판단한다. O는 가드 규칙에 걸리는 것만 먼저 막는다.
- 승인 서버는 Claude Code의 `--permission-prompt-tool` MCP 형식(2.1.302)과 Codex app-server 승인 스키마(0.166.0)에 기대며, 바뀌면 다시 확인한다.
- Windows의 Codex(0.166.0)는 읽기 전용 샌드박스에서도 `Get-Content` 같은 읽기 명령에 승인을 요청했다. 승인된 명령은 샌드박스 밖에서 돌기 때문에 O는 명령 분류(`commandActions`, 참고용 추정)를 믿고 자동 허용하지 않는다. Codex 총괄은 Windows에서 셸 읽기도 승인 카드로 묻는다.

### 확인 (2026-11-04, 홈 폴더 밖 `C:\tmp\o-approve-e2e`, 개발 빌드 `o lead send`)

| # | 총괄 | 요청 | 결정 | 결과 |
|---|---|---|---|---|
| 1 | Claude Code 2.1.302 `haiku` | hello.txt 만들기 | `o lead decide … allow` | `lead show`에 대기 요청(`'hello.txt' +1 −0`)이 보였고, 허용 뒤 파일 생성, `lead.step` edit `+1 −0 done` |
| 2 | 같은 세션 | bye.txt 만들기 | `deny` | 파일 없음, `lead.step` `denied`, 총괄이 재시도 없이 거부를 알림 |
| 3 | Codex 0.166.0 `gpt-5.6-luna` | codex.txt 만들기 | 명령(`Set-Content`) `allow`, 이어진 읽기(`Get-Content`) `deny` | 파일 생성, 읽기 단계 `denied`, 총괄이 재시도 없이 알림 |

## 영향

- 총괄은 사람이 하나씩 허용하지 않은 파일 수정·명령 실행을 할 수 없다(개정 3, 그전에는 아예 할 수 없었다). 카드 권한(ADR-0002)·정책 상한·예산(ADR-0004)은 바뀌지 않고, 계획은 `o dispatch`와 같은 길로만 실행된다.
- O 상태 폴더에 `leads/`, 그 옆에 `project_O-lead/`가 생긴다. 지워도 실행 기록과 색인에는 영향이 없다.
- 새 프로바이더를 총괄로 쓰려면 `lead_efforts`·`lead`를 구현하고 `crates/o-core/tests/providers.rs`에 정확한 인자와 "쓰기·우회 없음" 사례를 더한다(ARCHITECTURE.md).
- 진행 표시: 총괄 CLI의 도구 이벤트 중 최근 세 개와 도구 호출 횟수를 임시 상태로 보관하고 `lead show`의 `thread.progress`·`thread.progressCounts`로 전달한다. 횟수는 성공 여부나 현재 실행 중인 개수가 아니다. 턴이 끝나면 숨기며 기존 대화 이벤트와 인덱스는 바꾸지 않는다. 화면은 수신한 최종 답변을 짧게 나눠 표시한다. 이는 모델 토큰의 실시간 스트리밍은 아니다.
- 남은 일: 카드 질문(`o msg` → lead)에 총괄이 답하는 연결, 실제 토큰 단위 스트리밍, 총괄 재접속 시 끊긴 턴 회수.
- 되돌리기: 기능 단위 커밋이므로 되돌릴 수 있다. 저장된 스레드는 남지만 읽는 쪽이 없을 뿐이다.
