# 아키텍처

모듈이 어디에 있고, 데이터가 어떻게 흐르고, 새 기능을 어디에 더하는지 적는다. 구조를 정한 이유는 [ADR-0007](decisions/0007-module-structure.md), 작업 규칙과 정책 차단은 [AGENTS.md](../AGENTS.md)에 있다.

```
┌──────── 화면 app/src (React) ────────┐   ┌──── 총괄 (앱 총괄 채팅 · skills/o) ┐
│ shell · features · data · platform   │   │ 계획 제안 → 사람이 실행 → o dispatch │
└──────┬───────────────────────────────┘   └──────────────┬──────────────────┘
       │ Tauri 명령 / 개발 서버 /api                        │ o 명령
┌──────▼──────────────┐  ┌──────────────────────────────────▼──┐
│ app/src-tauri       │  │ crates/o-cli (o.exe)                 │
│ commands/<도메인>.rs │  │ commands/<명령>.rs                   │
└──────┬──────────────┘  └──────┬───────────────────────────────┘
       └──────────┬──────────────┘
          ┌───────▼────────┐        ┌──────────────┐
          │ crates/o-core  │        │ crates/o-guard│ 훅·저장소 검사 규칙
          └───────┬────────┘        └──────────────┘
                  │ 공식 CLI를 셸 없이 실행 (claude · codex · agy · opencode · devin)
```

## Rust

| 크레이트 | 역할 |
|---|---|
| `crates/o-guard` | 정책 규칙(`rules.rs`), 에이전트 훅(`o hook`), 저장소 검사(`o scan`) |
| `crates/o-core` | 판단과 상태를 모두 가진 코어. CLI와 데스크톱 셸이 같이 쓴다 |
| `crates/o-cli` | `o.exe`. 인자 해석과 출력만 한다 |
| `app/src-tauri` | `project_O.exe`. 화면이 부르는 Tauri 명령을 `o-core`로 처리한다 |

### `crates/o-core/src`

| 모듈 | 파일 | 하는 일 |
|---|---|---|
| `run/` | `plan` `events` `runner` (`runner/card`, 실행기 테스트) `store` `leases` `index` `snapshot` `team` `team_command` `msg_relay` | 계획 형식, 정규화 이벤트(`events.jsonl` 계약), 병렬 실행기와 카드 실행·자원 예약, 상태 폴더, 자원 예약 기록과 복구(`store::…`로 부른다), SQLite 색인 `o.db`, 화면용 스냅샷, 팀 대화, `o msg`의 동작(`team_command`)과 Codex 카드의 우편함 중계(`msg_relay` — 샌드박스 안의 `o msg`가 쓴 요청을 카드 실행기가 그 카드 이름으로 실행, ADR-0043) |
| `policy.rs` | | 계획·팀 메시지 허용 여부와 `LIMITS`. 공용 경계라 맨 위에 둔다 |
| `providers/` | `mod.rs` `claude.rs` `codex.rs` `codex_app_server.rs` `codex_sandbox.rs` `agy.rs` `opencode.rs` `devin.rs` `stream.rs` | `Provider` 트레이트와 등록부 `providers::get`. CLI마다 실행 인자·권한, 스트림 해석, 설치·로그인, 깨끗한 프로필(지원 여부 `profile_unsupported`, 카드 전에 필요한 설정·점검 `profile_not_ready` — Codex Windows 샌드박스는 `codex_sandbox`의 점검 기록(app-server 설정 + 실제 명령 한 번, Codex 버전·config 해시가 맞을 때만 준비됨), ADR-0040 개정 1), 사용량 조회. YOLO가 아닌 Codex 총괄은 `codex app-server`(읽기 전용 샌드박스, 승인 `on-request`를 사람에게, 훅 끔, ADR-0023 개정, app-server가 없는 CLI는 읽기 전용 `codex exec`), 카드와 YOLO 총괄은 `codex exec` |
| `agents/` | `mod.rs` `clean.rs` `billing.rs` `agy.rs` | CLI 설치·로그인과 테스트 프로필, API 과금 검사, Antigravity 작업 에이전트 동의와 붙여넣기 설정(ADR-0042) |
| `term/` | `mod.rs` | 호스트 세션·PTY 출력·입력·붙여넣기 승인·종료와 총괄 계획 감시. `o term`과 Tauri `term_*`가 공유한다(ADR-0042) |
| `usage/` | `mod.rs` `probe.rs` `budget.rs` `thread_total.rs` | 구독 창 형식, `usage.json` 조회·저장(실행이 쓰는 프로필로 조회), 하루 사용 예산(ADR-0004), Codex `exec` 스레드 누적에서 턴이 쓴 토큰만 세기(개정 2) |
| `context/` | `mod.rs` `tokens.rs` | 세션 공유 컨텍스트 검색·주입과 토큰 보고(ADR-0005) |
| `lead/` | `mod.rs` `event.rs` `turn.rs` `answer.rs` `worker_reply.rs` `dispatch.rs` `view.rs` `tabs.rs` `prompt.rs` `files.rs` `approvals.rs` `steps.rs` `overall.rs` `recovery.rs` `log_page.rs` | 총괄 탭(`tabs`, ADR-0034): `leads/open.json`에 프로젝트별 최대 4개 열린 대화와 writer 하나, `current.json`에 선택한 탭, `leads/<id>/events.jsonl`에 대화 기록. writer 외의 탭은 읽기 전용이며 서로 다른 탭은 동시에 답할 수 있다. 전체 총괄(`overall`, ADR-0032)은 프로젝트 루트의 읽기 전용 탭들로, 열린 대화 수·현재 탭 현황을 요약하고 `o-forward` 전달 시작 시 정한 열린 목적지를 유지한다. 전체 총괄의 `o-forward`는 제안(`lead.forward-plan`)으로 저장되어 사람의 `o lead forward`(`send_on`)로만 넘어가며, 넘긴 요청의 계획은 전체 총괄이 `o-review`로 검토해 승인하면 바로 실행하고 보완이면 최대 2번 다시 넘긴다(ADR-0036). 총괄 한 턴·세션 재개, `o-plan` 정책 검사·한 번만 실행, 첨부(O 상태 폴더 밖), 일반 모드 건별 승인(`approvals`, ADR-0008 개정 3)과 진행 단계(`steps`). 카드 질문은 실행기와 `recovery`가 분배한 열린 대화로 보내고, `log_page`는 열린 대화가 분배한 완료 실행의 원본 로그를 검증해 읽는다 |
| `graph/` | `wiring` `query` `view` `grafly` `git` `deep` `settings` | Graph 탭(ADR-0033): grafly 인덱스(`.grafly/.graph/wiring.json`)를 읽어 호출 관계·변경 영향·파일 API·개요를 LLM 없이 계산, 3D 화면용 그래프와 빛나는 경로(`git status` + 영향 범위), `grafly build/check/ask` 호출, 사람이 고른 CLI·모델로 영역 이름·요약을 한 번에 만드는 Deep, 설정(`graph/settings.json`) |
| `dream` | `dream.rs` | Wiki·Dreaming 탭(ADR-0041): 사람이 넣은 외부 자료(`<상태 폴더>/dreaming/external/`, 글·웹 주소)와 끝난 카드마다 실행 색인에서 읽는 작업 성공·실패 기록, 카드의 실패한 도구 호출(`card.tool.error`)을 모은 도구 오류 문서의 목록·본문·양방향 연결, 그것을 엮은 그래프(같은 실행·후속 실행·같이 바꾼 파일·글이 가리키는 자료)와 하루 안의 빛남. 문서의 태그와 태그로 찾기(`find`·`tags`), 바뀐 출처만 다시 읽는 문서 색인 `dream_index`, 카드의 읽기 범위(`Scope::Card` — `o context --wiki`)는 개정 2. LLM이 없고, 쓰는 것은 `add`·`remove`와 색인뿐이다 |
| `tuning` | `tuning.rs` | Dreaming의 조정(ADR-0041 개정 1): 실행 색인에서 되풀이된 도구 오류·작업 실패를 찾아 그 프로젝트의 카드 프롬프트에 더할 줄로 만들고(`section`, 러너가 카드 시작 때 부른다), `<상태 폴더>/dreaming/tuning.json`에 남겨 되돌리기·다시 적용을 받는다(`o dream changes|revert|restore`) |
| `review.rs` | | `o review`: PR을 gh로 읽어 검토 카드 하나로 실행(ADR-0006) |
| `sessions.rs` | | 이 PC의 Claude Code 세션을 실행처럼 읽기(읽기 전용) |
| `projects.rs` | | 앱 왼쪽 목록의 프로젝트(ADR-0009): 프로젝트 루트(기본 `~/.project_o/project`)의 하위 폴더, 그 `test` 묶음, 등록한 폴더(`projects.json`). 등록은 `policy::check_project`와 같은 검사 |
| `sys/` | `proc` `pty` `resolve` `js` `mirror` `agent_job` `win_launch` | 프로세스·Job Object·Windows ConPTY, 셸 없이 CLI 찾기, JS와 같은 시각·문자열 형식, 거울 동기화, 카드·총괄의 이름 있는 작업 개체와 그 안인지의 판정(`agent_job`, ADR-0043) |

Devin 카드는 project_O 프로필 계획에서도 평소 프로필을 쓰고 안내한다. Antigravity 총괄은 사람이 쓰는 터미널이며, 기본 꺼짐인 작업 에이전트는 동의 후 터미널 카드로 실행한다. `o agy`로 설정하고 `o term`으로 세션을 다룬다(ADR-0042).

순수 모듈(`run::plan`, `run::events`, `run::snapshot`, `policy`, 각 프로바이더의 스트림 해석)은 파일·프로세스를 건드리지 않는다.

### `crates/o-cli/src`

`main.rs`는 `commands::find`로 명령을 찾아 실행할 뿐이다. `commands/mod.rs`의 `COMMANDS` 표가 이름·도움말 줄·처리 함수를 가지며 `o help`는 이 표의 순서대로 만들어진다. 명령 파일: `dispatch`(check·dispatch·내부 `_run`), `status`, `events`, `stop`, `msg`, `lead`(send·show·new·attach·dispatch·decide·stop·models, 내부 `_lead-approve`), `review`(내부 `_review` 포함), `context`, `tokens`, `usage`, `budget`, `ram`, `billing`, `runs`, `sessions`, `projects`(list·add·remove·new·root·check), `guard`(hook·scan), `mirror`, `agents`, `about`.

숨김 명령 `o _job-watch`는 Windows 전용 `run::job_watch`를 실행한다. 실행기 종료 뒤 카드 Job을 종료하고 활성 프로세스 수 0과 보유한 프로세스 핸들의 종료 신호를 확인한 다음, 정확히 일치하는 자원 예약에 `exit_confirmed`를 남긴다([ADR-0039](decisions/0039-dead-runner-cleanup.md)).

`o agy`는 작업 에이전트 동의와 붙여넣기 설정을, `o term open|list|read|write|resize|paste|close`는 터미널 세션을 다룬다. 숨김 `o _term`은 총괄 터미널 호스트다(ADR-0042). Tauri 명령은 `commands/agy.rs`와 `commands/term.rs`에 있다.

터미널 입력은 세션 잠금으로 순서를 정하고 한 번에 최대 64 KiB를 받는다. 총괄 계획은 비어 있지 않은 파일이 1.5초, 카드 완료 문서는 500ms 동안 안정된 뒤 읽으며 계획 이름과 내용 해시로 재접수를 구분한다. 완료 문서는 `O-DONE` 첫 줄과 `O-END` 마지막 줄에 같은 일회용 값이 있어야 한다. 터미널 카드의 모델·노력 수준은 CLI 현재 설정을 쓰고 계획 검사에서 안내한다.

`o resources`와 상태 표시줄의 자원 예약 점검은 남은 예약을 보여 준다. 일반 복구는 종료 증거를 검사한다. 실행기와 작업 프로세스의 종료만 확인되고 자손 종료는 불명인 예약은 사람이 모든 자손의 종료를 직접 확인한 뒤 앱에서 확인하거나 `o resources recover <id> --confirm-exited`로 복구한다. 저장소는 복구 직전 트랜잭션에서 프로세스 신원을 다시 검사한다(ADR-0039 개정 3).

### `app/src-tauri/src`

`main.rs`는 `commands::handler()`를 등록하고 `commands::window::open_main`으로 메인 창을 띄운다(`tauri.conf.json`의 `create: false`). `commands/{runs,usage,agents,review,tokens,about,lead,projects,update,window}.rs`가 도메인별 명령과 테스트를 가진다. `projects`의 폴더 선택 창(`pick_folder`)은 `tauri-plugin-dialog`를 Rust에서만 연다(화면 capability 없음). `window`는 데스크톱 전용이다: 새 창을 여는 링크(`target="_blank"`, `window.open`)는 웹뷰 창을 만들지 않고 https만 `open_external`과 같은 경로로 브라우저에서 열며, 나머지는 막고 화면에 `project-o:link-blocked` 이벤트로 이유를 알린다(`shell/LinkNotice.tsx`). 팝아웃은 `popout_open`이 고정된 앱 주소(`index.html?popout=panel&source=live…`)로 Tauri 창을 만든다. 카드 완료 알림은 `tauri-plugin-notification`이 화면의 `Notification`을 Windows 알림으로 바꿔 보낸다. 플러그인 권한은 `capabilities/default.json`의 main 창 알림 3개뿐이다(앱 자체 명령은 권한 파일 없이 모든 창에 열려 있다). 모든 명령은 막히는 일을 별도 스레드에서 하고 셸을 쓰지 않는다. `lead`는 총괄 채팅(ADR-0008)이다. `update`는 데스크톱 전용으로 고정 저장소의 릴리스를 `gh`로 확인하며, 사람의 클릭 후에만 설치기를 연다. `uninstall.rs`는 NSIS가 쓰는 별도 데이터 제거 실행 파일이고 `installer_paths.rs`의 경로 검사를 공유한다.

## 화면 `app/src`

| 폴더 | 내용 |
|---|---|
| `shell/` | `App.tsx`(메인 창·팝아웃), `Sidebar`, `SidePanel`(오른쪽 패널과 보기 목록), `StatusBar`, `panelLayout.ts`, `announcer.ts`, 막힌 링크 안내 `LinkNotice`(`main.tsx`에 마운트) |
| `features/chat/` | 총괄 대화: 실제 스레드 `LeadThread`(계획 카드), 진행 단계 `LeadWork`(진행 중 목록·답 위의 접힌 "작업 n단계"), 입력창 바로 위 승인 카드 `LeadApprovalCard`(거부가 기본 포커스, 항상 허용 없음), 데모 `Thread`·`LeadGate`, 입력창 `Composer`, 카드 기록 `CardTranscript`, `TeamChat`, 채팅 글 그리기 `Markdown`(GFM, `react-markdown`)과 허용 규칙 `markdown.ts`(원시 HTML은 글자로, 링크는 http(s)만, 프로젝트나 작업 폴더 안의 로컬 이미지만 별도 표시, `o-plan` 블록은 계획 카드에 맡김) — 모양은 `styles/chat-markdown.css` |
| `features/office/` | 사무실 `Office`·`OfficeView`, `Person`, `Sprite`, `characters.ts`, `Avatar`, `WhoIsWorking` |
| `features/work/` | 에이전트 작업 탭 `WorkTab`(설계·대시보드), 팀 에이전트 탭·분할 화면 `AgentPanes`(1·2·4칸, 칸 배정은 `src/core/panes.ts`), 전체 `WorkDashboard`·`AllNextUp`, 예약 `Automation`, `Ticket`, `ReviewRequest` |
| `features/usage/` | `UsagePanel`, `TokenCard` |
| `features/terminal/` | 터미널 바이트를 xterm에 표시하는 `TerminalView`, Antigravity 총괄과 계획 카드를 잇는 `LeadTerminal`, 세션 목록 `useTermSessions`, 지연 로딩 `xtermLib` |
| `features/graph/` | Graph 탭(ADR-0033): `GraphScreen`(조립), `GraphToolbar`(효과·흐름 스위치, 인덱스 만들기, Deep), `ScopeList`(전체 → 프로젝트), `CanvasState`(불러오기·오류·빈 상태), `NodeDetail`, `Graph3D`(3D 캔버스의 React 껍데기)와 `graphScene`(노드·링크·빛을 묶어 그리는 렌더러, ADR-0041 — `3d-force-graph`는 캔버스·카메라만 맡고 `three`와 함께 필요할 때만 불러온다), 배치 계산 `graphLayout`과 그것을 화면 밖에서 돌리는 `layoutWorker`, 첫 Deep 안내 `DeepConfirm`, 순수 모듈 `graphStyle`·`graphMotion`(등장 시점, 카메라 거리, 노드 고르기)·`graphText`(HTML 이스케이프, 숫자 표시)·`modelTier`, 불러오기 `useGraphView` |
| `features/dreaming/` | Dreaming 화면(ADR-0041): `DreamingScreen`(Graph·Wiki·Dreaming 탭과 열린 문서), `DreamView`(외부 자료·작업 기록 그래프 — `Graph3D`를 같이 쓴다) |
| `features/wiki/` | Wiki 탭(ADR-0041): `WikiScreen`(폴더 트리, 문서 본문, 양방향 연결, 자료 빼기), `AddMaterial`(글·파일·웹 주소로 외부 자료 넣기), 순수 모듈 `wikiTree`(트리, 찾기, 링크가 가리키는 문서) |
| `features/projects/` | 새 요청 화면 `NewRequest`(맡길 프로젝트 고르기·새 프로젝트·폴더 등록), 첫 실행 `ImportCard`(기록에서 찾은 프로젝트), 공용 양식 `ProjectForms` (ADR-0009) |
| `features/updates/` | `UpdatePrompt`(shell에 한 번 마운트), 정보 항목의 `UpdateSettings`. 공용 업데이트 타입·문구·제거 명령은 `platform/updates.ts` |
| `features/settings/` | `Settings`(틀), `Section.tsx`(항목 계약·공용 배치), `sections/*.tsx`(항목별 파일, `sections/index.ts`가 순서), `AccountMenu`, `AgentSetup`, `Onboarding` |
| `data/` | 실행 출처 `useLiveRuns.ts`(실제, 데스크톱 앱은 데모로 넘어가지 않음)·`useRun.ts`(데모·URL 상태), 총괄 스레드 `useLead.ts`, `projects.ts`, `settings.ts`(이 브라우저의 설정), 데모 기록 JSON |
| `platform/backend/` | `Backend` 계약 `types.ts`와 구현 셋: `tauri.ts`, `dev.ts`, `demo.ts`. `index.ts`의 `backend()`가 고른다 |
| `lib/` | `format`, `storage`, `timeScale`, `dismiss` |
| `ui/` | 여러 기능이 쓰는 컨트롤(`controls.tsx`), 위로 열리는 선택 메뉴 `Menu.tsx`, `AgentStateDot`, 바깥 링크 `ExternalAnchor`(데스크톱은 `open_external`, 브라우저는 noopener 새 탭 — 채팅 Markdown 밖의 모든 바깥 링크) |

저장소 루트의 `src/core/*.ts`(기록 접기 `snapshot`, 다음 할 일 `attention`, 이벤트 타입, 한도 표시값, 총괄 스레드 접기·노력 수준·첨부 검사 `lead`, 진행 단계 문구·승인 카드 표시값 `leadWork`, 프로젝트 소속·첫 실행 후보·카드 칩 요약 `projects`, 에이전트 칸 배정 `panes`)는 화면과 `tests/`가 같이 쓰는 순수 로직이다. 화면은 판단하지 않고 이 함수의 결과를 그린다.

## 데이터 흐름

0. **총괄 채팅**(ADR-0008): 앱 입력창의 글과 첨부가 `o lead send`(Tauri `lead_send`)로 가면 `lead`가 그 프로젝트의 스레드에 남기고, 등록부의 `Provider::lead` 인자로 총괄 CLI를 한 턴 띄운다(세션 재개, 인증 환경은 ADR-0035). 읽기 외의 동작은 `lead::approvals`가 가드 검사 뒤 승인 카드(`thread.approvals`)로 사람에게 묻고, `o lead decide`(Tauri `lead_decide`)의 답만 받는다. 스트림의 도구 호출은 `lead::steps`가 `lead.step`으로 남긴다(개정 3). 답의 `o-plan` 블록은 프로젝트를 고정해 2단계와 같은 검사를 받고 `lead.plan`으로 저장된다. 사람이 계획 카드의 "실행"을 누르면 `o lead dispatch`가 다시 검사해 3단계로 넘긴다.
1. **계획**: 총괄(앱 총괄 채팅, 또는 `skills/o`를 쓰는 터미널 에이전트)이 계획 JSON(프로젝트 하나, 카드 몇 장)을 만든다. 카드에는 데이터만 있다(`run::plan`).
2. **정책**: `policy::check_plan`이 `run::store::policy_context`(오늘 카드 수·실행 중 카드·쿨다운·하루 예산)로 계획을 검사한다. 위반이 하나라도 있으면 아무것도 실행하지 않는다(종료 코드 2).
3. **실행**: 통과하면 `runs/<runId>/`를 만들고 `o _run`을 백그라운드로 띄운다. `run::runner`는 카드마다 시작 직전에 상한·예산을 다시 확인하고, `context::prepare`로 관련 컨텍스트 절을 만든 뒤 `providers::launch_spec_with`의 인자로 공식 CLI를 셸 없이 띄운다(`--profile project-o`·`--clean`이면 `agents::clean` 환경, ADR-0013). Windows에서는 Job Object로 묶는다.
4. **이벤트**: CLI 표준 출력 한 줄마다 `providers::StreamReader`가 그 CLI의 해석기로 `StreamSignal`을 만들고, 실행기가 이를 `OEvent`로 바꿔 `events.jsonl`에 덧붙인다. 팀 대화(`o msg`)도 같은 파일에 정책 검사를 거쳐 들어간다.
5. **색인**: `run::index`가 `events.jsonl`을 `o.db`에 증분으로 넣고, `context`가 같은 DB에 검색 표를 유지한다. `events.jsonl`이 원본이며 색인은 언제든 다시 만들 수 있다.
6. **앱**: `o runs --json`(개발 서버 `/api/runs`)이나 Tauri `runs`가 실행별 이벤트를 주면 `data/useLiveRuns.ts`가 2초마다 읽고, `src/core/snapshot.ts`로 접어 화면이 그린다. 왼쪽 목록은 `o projects --json`(Tauri `projects_list`)의 프로젝트만 보이고, 실행·세션·총괄 대화는 그 폴더 안일 때 붙는다(`data/projects.ts`, ADR-0009). `o status`는 같은 접기를 Rust `run::snapshot`으로 한다.

곁가지: `o usage`(`usage::probe`)는 각 프로바이더의 `usage_windows`로 구독 창을 읽어 `usage.json`에 저장하고, `usage::budget`이 그 기록으로 하루 예산을 계산해 2단계 정책에 쓴다.

## 새 기능 추가하기

### (a) 에이전트 CLI 프로바이더

1. `crates/o-core/src/providers/<cli>.rs`를 만들고 `Provider`를 구현한다: `launch`(권한 확인을 끄는 옵션 금지, 허용 명령은 `o msg`·`o context`만), 인증 환경변수 목록(깨끗한 프로필과 과금 경고에 사용, ADR-0035), `tool_note`, 카드 안 서브에이전트를 쓸 수 있으면 `subagent_tool`(ADR-0037), `searches_context`, `stream_reader`, `official`, `login_args`·`status_args`·`read_login`, `clean_env`·`prepare_profile`·`inspect_profile`, 필요하면 `usage_windows`와 프로필에서 카드 전에 마칠 설정·점검이 있을 때 `profile_not_ready`(ADR-0040 개정 1).
2. 등록: `run/plan.rs`의 `ProviderId`에 변형을 더하고(`ALL`·`as_str` 포함) `providers/mod.rs`의 `get`에 한 줄을 더한다. 컴파일러가 빠진 `match`를 알려 준다.
3. `policy.rs`의 `max_active_per_provider`와 화면 사본 `src/core/limits.ts`는 공용 경계다. ADR과 두 사람 승인 뒤에 바꾼다(`tests/limits.test.ts`가 둘을 비교한다).
4. 화면: `src/core/plan.ts`의 `PROVIDERS`, `platform/backend/types.ts`의 `AgentId`에 id를 더하면 TypeScript가 `PROVIDER_NAME`·`Avatar` 등 빠진 표를 알려 준다.
5. 테스트: `crates/o-core/tests/providers.rs`에 정확한 실행 인자와 스트림 해석 사례를, `clean.rs`에 프로필 검사를 더한다.

**총괄을 맡을 수 있는 CLI로 만들기**(ADR-0008): 헤드리스로 읽기 전용 실행과 세션 재개를 모두 보장할 수 있을 때만 한다.

1. `Provider::lead_efforts`에 그 CLI가 받는 노력 수준을, `Provider::lead`에 `LeadTurn`(프로젝트·프롬프트·재개할 세션·모델·노력·첨부 폴더·이미지)으로 만든 실행 인자를 둔다. 쓰기·셸 도구와 권한 확인을 끄는 옵션은 넣지 않는다. 모드 이름이 아니라 CLI가 강제하는 장치(도구 목록, 샌드박스, 거부 규칙)로 읽기 전용을 만들고, 홈 폴더 밖 임시 폴더에서 고치기·셸 요청이 막히는지 실제로 확인한다(ADR-0008 개정 1). 스트림은 카드와 같은 `stream_reader`로 읽으므로 세션 id(`StreamSignal::Session`)와 최종 답을 내야 한다.
2. 그 CLI의 인증 환경변수를 깨끗한 프로필과 과금 경고에 반영한다(ADR-0035).
3. `Provider::lead_models`에 그 CLI의 공식 모델 목록(목록 명령은 `providers::list_command`로, 없으면 문서화된 별칭)을 둔다.
4. 화면 사본 `src/core/lead.ts`의 `LEAD_EFFORTS`·`LEAD_NAME`에 같은 값을 더한다(`tests/lead.test.ts`가 Rust와 비교한다).
5. 테스트: `providers.rs`에 첫 턴·재개 인자와 "쓰기·우회 없음" 사례(`lead_launches_never_write_and_never_turn_approvals_off`가 모든 프로바이더를 돈다), 모델 목록 해석 사례.

### (b) 화면 패널 · 작업 탭 하위 보기 · 설정 항목

- **오른쪽 패널 보기**: 기능 폴더에 `PanelViewDef`(이름·아이콘·`render`)를 내보내는 파일을 만들고(예: `features/office/OfficeView.tsx`), `data/useRun.ts`의 `PANEL_VIEWS`에 id를, `shell/SidePanel.tsx`의 `VIEWS`에 한 줄을 더한다. `?view=` 링크도 이 목록으로 읽는다.
- **에이전트 작업 탭 하위 보기**: 보기 컴포넌트 파일을 만들고 `features/work/WorkTab.tsx`의 `Sub` 유니온과 `SUBS` 표에 한 줄을 더한다(배지·검토 요청 양식 여부도 그 줄에서 정한다).
- **설정 항목**: `features/settings/sections/<이름>.tsx`에 `SettingsSectionDef`를 `as const satisfies`로 내보내고 `sections/index.ts`의 그룹에 한 줄을 더한다. 항목 id 유니온은 이 목록에서 만들어진다.

### (c) `o` 하위 명령

1. `crates/o-cli/src/commands/<이름>.rs`에 `pub const <NAME>: Command = Command { name, usage, run }`을 둔다. `usage`는 `o help`에 보일 줄(내부 명령은 빈 문자열), `run`은 `(home, args) -> 종료 코드`.
2. `commands/mod.rs`에 `mod <이름>;`과 `COMMANDS`의 한 줄을 더한다. 표의 위치가 도움말 순서다.
3. 판단·상태 변경은 `o-core`에 두고 명령 파일은 인자 해석과 출력만 한다. `crates/o-cli/tests/`에 실제 `o` 실행 테스트를 더한다.

### (d) Tauri 명령과 개발 서버 `/api` 쌍

1. `app/src-tauri/src/commands/<도메인>.rs`에 `#[tauri::command]` 함수를 두고(새 도메인이면 파일과 `mod` 한 줄), `commands/mod.rs`의 `handler()` 목록에 한 줄을 더한다. 함수 이름이 화면에서 부르는 명령 이름이다.
2. 같은 동작을 `o` 명령으로도 둔다(c). 개발 서버는 `o`만 부른다: `app/dev/liveRuns.ts`에 경로 한 줄을 더한다(이 PC 주소·같은 출처 검사 뒤). 사람 전용 agy 설정 변경과 터미널 열기·입력·붙여넣기·닫기는 Origin을 위조할 수 있어 개발 서버에 제공하지 않는다.
3. 화면: `platform/backend/types.ts`의 `Backend`에 메서드를 더하면 `tauri.ts`·`dev.ts`·`demo.ts`가 각자 구현하도록 TypeScript가 강제한다. 실행 목록·사용량 폴링은 `data/useLiveRuns.ts`가 같은 명령을 직접 부른다.
4. 명령은 고정 인자만 받고 프로세스를 직접 띄우지 않는다. 사용자 입력(경로·PR 번호 등)은 명령 쪽에서 다시 검사한다.

데스크톱 앱 자체의 업데이트는 CLI 작업카드에 노출하지 않는다. `update` 명령의 개발 서버 `/api/updates`와 데모 구현은 데스크톱 전용 안내를 반환하며 `o` 하위 명령은 없다.

## 플러그인

지금의 확장 지점은 모두 **컴파일 시점**이다: 프로바이더 등록부(`providers::get`), `o` 명령 표(`COMMANDS`), Tauri `handler()`, 화면의 패널 보기·작업 탭 하위 보기·설정 항목 목록, `Backend` 구현 셋. 모두 일반 배열·`match`이고 실행 중에 코드를 불러오는 장치는 없다.

제3자 플러그인을 실행 중에 불러오는 기능은 두지 않는다. 카드 권한(`o msg`·`o context`만), 깨끗한 환경 허용 목록, 셸 없는 실행, 인증 파일 비접근 같은 보안 경계를 플러그인이 우회할 수 있기 때문이다. 필요해지면 권한 모델(서명·격리·허용 API)을 먼저 정하는 별도 ADR로 다룬다.

## 바꿀 때 지키는 것

- 프로바이더 보안 동작(허용 도구 `Bash(o msg *)`·`Bash(o context *)`, Codex `--add-dir`(카드 우편함만), agy의 인자 없는 터미널 카드, 깨끗한 환경 허용 목록, 권한 우회 옵션 없음)은 `crates/o-core/tests/{providers,clean,agents,runner}.rs`가 고정한다. 총괄의 읽기 전용 실행·한 번만 실행·첨부 규칙·인증 환경 전달는 `crates/o-core/tests/{providers,lead}.rs`가, 일반 모드 승인(허용·거부·시간 초과·중지·가드 거부·자기 승인 차단·Codex 응답 매핑)과 YOLO 불변은 `lead/approvals.rs`·`tests/lead_approvals.rs`·`providers.rs`(`yolo_leads_ignore_the_approver`)·`o-cli/tests/agent_boundary.rs`가 고정한다.
- `policy.rs`의 `LIMITS`, `crates/o-guard/src/rules.rs`의 규칙·허용 경로는 공용 경계다(ADR + 두 사람 승인).
- `events.jsonl`의 이벤트 형식은 화면 계약이다(`src/core/events.ts`). 새 이벤트는 모르는 화면이 무시할 수 있게 더한다.
- `o help`·명령 출력·종료 코드는 총괄 에이전트와 개발 서버가 읽는다. 구조를 바꿔도 그대로 둔다.
