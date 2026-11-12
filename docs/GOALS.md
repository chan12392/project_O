# 제품 목표와 현재 상태

> 2026-11-03: Antigravity 실행과 자동 조회는 공급자 정책에 따라 보류했다([ADR-0020](decisions/0020-antigravity-policy.md)). 아래 Antigravity 관련 실행 현황은 이전 기준의 기록이다.

기준: 2026-11-02, `<개인>-goals`의 문서 작성 전 HEAD `77a69aa`.
현재 상태는 이 기준 소스를 읽어 확인한 구현이며, 실사용 검증 완료를 뜻하지 않는다.
병렬 작업은 담당 범위만 전달받았으므로 아래의 구현 현황과 구분한다.

## 중심 목표: 토큰 사용 최적화

매번 다른 에이전트에게 일을 나누면 AGENTS·USER·MEMORY 등의 기본 컨텍스트가 다시 주입된다.
깨끗한 에이전트와 세션 공유를 결합해, 각 작업에 필요한 컨텍스트만 읽히는 것이 중심 목표다.
여섯 기능은 이 목표를 지원한다. 시각화는 낭비·정체를 찾고, NextUP은 재탐색을 줄이며,
검토·예산·공유·분담은 같은 품질의 결과를 더 적은 토큰으로 얻도록 해야 한다.
현재는 실행·관측·격리 기반이 있고, 필요한 문맥의 선택 주입과 절감 효과 측정은 남아 있다.

## 공통 기반과 판단 기준

| 기반 | 현재 코드와 범위 |
|---|---|
| 계획·실행 | `crates/o-core/src/run/plan.rs`, `run/runner.rs`, `policy.rs`: 카드 검사, 공식 CLI 실행, 대기·중지·결과 기록 |
| 기록·조회 | `crates/o-core/src/run/store.rs`, `index.rs`: 실행별 `events.jsonl` 원본과 SQLite `o.db` 증분 색인 |
| 화면 상태 | `src/core/snapshot.ts`: 이벤트를 카드·대화·사용량으로 접음; CLI 상태 조회에는 Rust `crates/o-core/src/run/snapshot.rs` 사용 |
| 앱 연결 | `app/src-tauri/src/commands/`는 `o-core` 호출, 개발 서버 `app/dev/liveRuns.ts`는 `o` 호출 |
| 정책 | `crates/o-guard`의 훅·정적 검사와 `o-core/src/policy.rs`의 실행 검사가 담당 |

- [ADR-0001](decisions/0001-stack-tauri-rust-core.md): Tauri 2·Rust 코어·React 채택, A 승인·B 사후 확인 필요 상태다.
- [ADR-0002](decisions/0002-security-hardening.md): 보안 경계 강화는 코드에 있으나 문서 상태는 제안, B 승인 대기다.
- [ADR-0003](decisions/0003-team-flow-test1-test2.md): 대표 결정으로 두 단계 통합 흐름이 승인됐으며 B 체크는 비어 있다.
- 새 공용 데이터 모델·프로바이더 연결·라우팅·최상위 화면·정책 변경은 [AGENTS.md](../AGENTS.md)의 ADR·승인 규칙을 따른다.
- [DESIGN.md](DESIGN.md)는 설계 초안이다. 과거 TypeScript 경로·예정 사항은 현재 구현의 증거로 쓰지 않는다.

## 1. AI오피스

> 여러 LLM을 사용하거나 단일 LLM 사용자의 에이전트 및 서브에이전트 시각화.

현재 있는 것:

- `crates/o-core/src/run/plan.rs`, `providers/`: Claude·Codex·Antigravity 카드를 같은 계획에 담거나 한 프로바이더만 사용할 수 있다.
- `app/src/features/office/Office.tsx`: 선택한 실행의 총괄과 카드들을 캐릭터로 표시하고, 상태에 따라 책상·대기·휴식 위치를 바꾼다.
- `app/src/features/work/WorkDashboard.tsx`: 프로젝트별 또는 전체 작업 흐름과 활동을 표시한다.
- `app/src/features/chat/CardTranscript.tsx`, `TeamChat.tsx`: 카드 기록과 같은 실행 안의 팀 대화를 표시한다.
- `app/src/data/useLiveRuns.ts`: Tauri/API에서 최근 실행 50개를 읽고 보이는 동안 2초 간격으로 갱신한다.
- 같은 파일에서 브라우저 미리보기는 소스를 지정하지 않았고 실제 실행이 없으면 데모 기록으로 전환한다. 데스크톱 앱은 전환하지 않는다(ADR-0008). 데모 화면은 실제 작업의 증거가 아니다.
- `crates/o-core/src/run/events.rs`에 `card.session`은 있지만 부모·자식 에이전트 관계 필드는 없다.

진행 중: `<개인>-office-nextup`이 목표 1–2를 담당한다. 이 기준 소스에는 그 결과가 반영되지 않았다.

남은 순서:

1. O 카드와 공식 CLI 내부 서브에이전트의 식별자·부모 관계·수집 가능 범위를 정하고 이벤트 계약을 ADR로 합의한다.
2. 한 프로바이더의 부모 1개·자식 1개 기록부터 수집해 사무실과 카드 기록을 연결한다.
3. 단일/복수 프로바이더와 종료·실패 상태를 검증한 뒤 외부 세션 수집 범위를 넓힌다.

현재 관측 대상은 O 실행 기록이다. 임의의 외부 CLI·Orca 세션 전체나 CLI 내부 서브에이전트 트리를 자동 수집하지 않는다.

## 2. NextUP

> 여러 프로젝트를 관리하기에 진행하다 멈춘 작업, 다음에 하면 좋을 작업을 추천.

현재 있는 것:

- `app/src/data/projects.ts`: 실행의 프로젝트 경로로 묶어 최근 활동순으로 나열하고, 사용자 그룹은 브라우저에 저장한다.
- `src/core/attention.ts`의 `nextUp`: 미응답 총괄 메시지, 완료 카드의 PR 링크, 24시간 내 완료 실행, 대기 카드를 뽑는다.
- 같은 함수가 실패·중지·한도와 실행 중 10분 넘게 자기 기록이 없는 카드를 구분하고, 종류와 대기 시간으로 정렬한다.
- `app/src/features/work/WorkDashboard.tsx`, `NextUp.tsx`: 기다리는 일·멈춘 일, 실행 흐름과 활동을 보여 준다.
- NextUp 티켓은 기록 열기, PR 링크, 답장·중지·상태 명령 복사와 처리 표시를 지원한다.
- `app/src/features/work/WorkTab.tsx`, `app/src/shell/SidePanel.tsx`: 실제 NextUp 입력은 선택한 프로젝트의 실행들이다.

진행 중: `<개인>-office-nextup`. 현재 구현은 기록에서 조건에 맞는 일을 추리는 규칙 기반 목록이다.

남은 순서:

1. 여러 프로젝트의 미처리 항목을 함께 비교하는 조회 경로부터 연결하고, 최근 50개 밖의 오래 멈춘 작업도 놓치지 않게 한다.
2. 추천 이유·원래 작업·필요한 문맥을 묶은 최소 재개 자료를 목표 5의 검색 결과와 연결한다.
3. 재개/새 카드 전달을 목표 6의 실행 경로에 연결하고, 처리 표시·중복·오래된 추천을 검증한다.

아직 작업 의미·의존성·예산을 판단해 새 후속 작업을 추천하거나, 멈춘 CLI 세션을 그대로 재개하지 않는다.
처리 표시는 `NextUp.tsx`의 로컬 저장소에 최대 500개만 보존하며 서버의 작업 완료 상태를 바꾸지 않는다.

## 3. AI와 인간의 공동 작업

> AI와 인간이 한 프로젝트에서 같이 작업 (에이전트 PR을 인간이 검토, 인간 PR을 에이전트가 검토).

현재 있는 것:

- `src/core/attention.ts`의 `prOf`는 카드 결과·로그에서 첫 GitHub PR URL을 찾고, 완료 카드에 검토 티켓을 만든다.
- `app/src/features/chat/CardTranscript.tsx`, `AllNextUp.tsx`는 그 링크와 PR 검토 상태를 표시한다.
- `crates/o-core/src/run/team.rs`, `crates/o-core/src/policy.rs`: 같은 실행의 카드와 총괄이 `o msg`로 대화하고 발신자·수신자·내용을 검사한다.
- `crates/o-guard/src/hook.rs`, `scan.rs`: 보호 브랜치 직접 변경과 규칙 밖 병합을 해당 훅 경로에서 막는다.
- `.github/workflows/branch-flow.yml`, `merge-guard.yml`: PR 방향 검사와 대표 외 보호 브랜치 병합·push 경보가 있다.

진행 중: `<개인>-pr-review`가 목표 3과 `o review`를 담당한다. 현재 `crates/o-cli/src/commands/`에는 이 명령이 없다.

남은 순서:

1. PR 번호·검토할 head 커밋·작성 주체를 확인해 검토 카드를 만드는 `o review`의 실제 동작과 검증 결과를 통합한다.
2. 인간 PR → 에이전트 검토 결과, 에이전트 PR → 인간 확인을 각각 연결하고 새 커밋이 생기면 이전 검토와 구분한다.
3. PR의 실제 열림·병합·검토 상태를 NextUP에 반영해 이미 처리한 PR이 계속 검토 대상으로 남지 않게 한다.

현재의 PR 표시는 링크 탐지다. 작성자 분류·diff 검토·검토 요청·리뷰 게시·상태 동기화는 구현돼 있지 않다.
브랜치 흐름은 개인/작업 → `test_2` → `test_1` → `main`, merge commit만 사용한다.
ADR-0003의 CI 경보는 사후 감지이며 GitHub 웹 병합을 사전에 강제 차단하지 못한다.

## 4. 사용량 최적화

> 하루 사용 예산을 주간의 12~14%로, 남은 시간을 계산해 effort 자동 조절.

현재 있는 것:

- `crates/o-core/src/usage/mod.rs`, `probe.rs`: 공식 CLI를 통해 Codex·Antigravity의 구독 창 사용 비율과 초기화 시각을 조회한다.
- `crates/o-core/src/providers/`: Claude 실행 스트림의 구독 창을 `quota`로, 세 프로바이더의 토큰을 `card.usage`로 정규화한다.
- `app/src/features/usage/UsagePanel.tsx`: 읽어 온 실행의 토큰 합계와 5시간·주간 구독 창을 표시한다.
- `crates/o-core/src/policy.rs`: 계획당 8장, 전체 동시 6개, Claude/Codex 각 3개·Antigravity 2개 상한이 있다. 날짜별 카드 수 제한은 없다(ADR-0045).
- `crates/o-core/src/run/runner.rs`: 시작 전 상한·쿨다운을 확인하고, 실행 중 한도 감지 시 같은 프로바이더의 대기 카드도 실패 처리한다.
- `crates/o-core/src/run/plan.rs`, `providers/`: 카드에 지정된 `effort`를 검증·전달한다. `skills/o/SKILL.md`는 총괄에게 사용량 확인과 선택을 안내한다.

진행 중: `<개인>-budget`이 목표 4를 담당한다. 카드 수 상한과 주간 사용량 비율 예산은 서로 다른 기준이다.

남은 순서:

1. 12~14% 중 적용값, 하루 경계, 주간 초기화, 프로바이더/모델 그룹별 계산 단위와 남은 시간의 의미를 합의한다.
2. `quota` 관측값의 신선도·초기화·누락을 처리한 예산 계산을 연결하고, 추천/선택한 effort와 이유를 기록한다.
3. 사용자가 지정한 모델·effort와 기존 정책 상한을 보존하면서 자동 조절 경로를 검증한다.

현재는 주간 예산 배분·남은 시간 기반 effort 자동 선택·예산 소진 예측이 없다.
토큰 개수만으로 구독 한도의 남은 비율을 환산하지 않는다. 한도 조회가 없으면 추정과 관측을 구분해야 한다.

## 5. 세션 공유

> 세션 컨텍스트를 따로 관리해 모든 에이전트가 프로젝트 전체 세션에서 필요한 부분만 읽고 빠르게 작업.

현재 있는 것:

- `crates/o-core/src/run/store.rs`, `runner.rs`: 실행 계획, 정규화 이벤트, 카드별 원시 출력과 오류 로그를 로컬에 남긴다.
- `crates/o-core/src/run/index.rs`: JSONL의 새 완결 행만 SQLite에 반영하며 실행·이벤트·시작 수·쿨다운·차단을 관리한다.
- `crates/o-cli/src/commands/runs.rs`: `o runs --json --project <폴더> --limit N`으로 프로젝트별 실행 기록을 조회할 수 있다.
- `crates/o-core/src/run/team.rs`: `o msg read`는 같은 실행 안의 새 메시지를 읽는다. 프로젝트 전체 세션 검색 명령은 아니다.
- `crates/o-core/src/agents/clean.rs`: 별도 홈·환경변수로 개인 훅·전역 지침·스킬·MCP·메모리·설정을 분리하고 `o agents doctor`로 점검한다.
- `o dispatch --clean plan.json`과 `run/runner.rs`가 깨끗한 프로필 실행을 연결한다. 프로젝트 지침·상위 폴더 지침까지 모두 제거하는 것은 아니다.

진행 중: `<개인>-context`가 목표 5와 토큰 보고 명령 `o tokens`를 담당한다. 현재 CLI에는 `o tokens`가 없다.

남은 순서:

1. 프로젝트·세션·원본 이벤트 위치를 보존하는 최소 검색 결과 형식과 읽기 권한을 ADR로 정한다.
2. 같은 프로젝트의 필요한 기록만 조회하고, 카드 프롬프트에 출처와 길이 제한을 둔 문맥을 주입한다.
3. 무관한 기록·오래된 결정·다른 프로젝트 정보가 섞이지 않는지 확인한 뒤 clean + 공유의 토큰 절감을 측정한다.

SQLite 색인은 현재 실행 목록/이벤트 조회용이며 의미 검색·요약·선택 주입 기능은 없다.
`card.session` 기록만으로 프로바이더 세션을 재개하거나 다른 에이전트에 문맥이 전달되지는 않는다.
깨끗한 프로필과 공유 문맥은 함께 검증해야 한다. 필요한 프로젝트 규칙까지 빠져 결과가 틀리면 최적화 성공이 아니다.

## 6. 하네스

> 총괄 에이전트 설정, 총괄이 각 에이전트에게 임무 분담(O CLI가 자동 분배), 서브에이전트와 다르게 동작.

현재 있는 것:

- `skills/o/SKILL.md`: 총괄이 파일 소유권이 겹치지 않도록 카드를 나누고 provider·model·effort를 고르는 절차가 있다.
- `crates/o-core/src/providers/`: 카드 목표·동료 목록·팀 대화 방법·결과 요약 지시를 짧은 프롬프트로 만든다.
- `crates/o-core/src/policy.rs`, `run/runner.rs`: 총괄이 만든 계획을 검사하고 각 카드를 독립된 공식 CLI 프로세스로 실행한다.
- O 카드는 특정 CLI의 내장 서브에이전트 호출이 아니다. 별도 CLI 세션이 같은 프로젝트 폴더에서 일하며 실행별 팀 채널로 연락한다.
- `app/src/features/settings/Onboarding.tsx`, `Settings.tsx`, `AgentSetup.tsx`: 공식 CLI 상태 확인·설치·로그인과 깨끗한 프로필 선택이 있다.
- `app/src/data/settings.ts`의 clean(설정 "project_O 프로필 사용") 기본값은 true이고, 상태 확인·로그인과 카드·총괄 실행에 전달된다. CLI도 기본이 `--profile project-o`다(ADR-0013).

총괄 채팅(`<개인>-lead`, [ADR-0008](decisions/0008-lead-chat.md)) — 남은 순서 1–3 중 1·2와 3의 일부를 구현했다:

- `crates/o-core/src/lead/`: 프로젝트별 총괄 스레드(`leads/<id>/events.jsonl`, "새 대화"), Claude·Codex 총괄의 읽기 전용 한 턴과 세션 재개(`Provider::lead`), 계획 블록(` ```o-plan `)을 읽어 프로젝트를 고정하고 `o dispatch`와 같은 정책으로 검사해 저장, 사람이 누른 "실행"만 다시 검사해 한 번 dispatch, 첨부(이미지·문서, O 상태 폴더 밖), 총괄 토큰을 `o tokens`의 부대 비용으로.
- `o lead send|show|new|attach|dispatch|stop`, Tauri `lead_*`, 개발 서버 `/api/lead/*`. 앱 입력창(에이전트·노력 선택, 첨부, 중지)과 계획 카드, 설정 → 총괄. 설정의 "project_O 프로필 사용"이 총괄 실행과 계획 dispatch(`--profile project-o|own`)에 실제로 전달된다.
- Antigravity는 읽기 전용 모드를 보장하지 못해 총괄에서 뺐다(카드로는 그대로 쓴다). 카드의 provider·model·effort는 총괄이 고른다.
- 실제 확인(2026-11-02): 로그인된 Claude CLI로 홈 밖 임시 프로젝트에 `o lead send --effort low`를 보내 카드 1장 계획이 저장되고 정책을 통과했다(실행은 하지 않음). 총괄 한 턴 입력 4,234 · 캐시 3,223 · 출력 567 토큰.
- 데스크톱 앱은 실제 데이터만 보인다. 녹화된 데모는 브라우저 미리보기(`?source=demo`, 개발 서버)에만 있다.

남은 순서:

1. 카드 질문(`o msg … --to lead`)에 앱 총괄이 답하는 연결: 지금은 다음 턴 프롬프트에 실행 상태와 결과 요약만 넣는다.
2. 총괄 답의 스트리밍 중간 표시, 앱이 꺼진 사이 끊긴 턴의 회수, 실패한 카드의 재시도 계획.
3. 같은 요청으로 터미널 총괄과 앱 총괄의 계획 품질·토큰을 비교한다.

O는 계획된 카드를 실행·대기시킨다. 작업 분해와 프로바이더·모델·effort 선택은 총괄 CLI가 제안하고, 사람이 실행을 결정한다.

## 병렬 작업의 통합 상태

네 작업 브랜치는 `<개인>-features`에 merge commit으로 합쳐졌다(goals → pr-review → budget → context → office-nextup). 통합 검토(Codex)의 높은·중간 우선 지적은 같은 브랜치에서 고쳤다.

| 브랜치 | 담당 | 통합 상태 | 통합 때 고친 것 |
|---|---|---|---|
| `<개인>-budget` | 목표 4 | 병합됨 — `o budget`, `daily-budget` 정책, `card.budget`, 앱 "오늘" 고리 | 주간 창이 초기화된 뒤 이전 창 사용량으로 계속 막던 문제, 자정 이전 기록 없이 당일 창 전환을 놓치던 문제 |
| `<개인>-context` | 목표 5, `o tokens` | 병합됨 — FTS5 검색·카드 주입·`o context`·`run.context`·`o tokens --compare`·`dispatch --no-context` | 카드의 `o context`를 실행의 프로젝트로 고정(다른 프로젝트·미확정 실행 거부), 주입 조각을 비신뢰 참고 자료 구획에 넣음, 1,500자를 절 전체 상한으로 |
| `<개인>-office-nextup` | 목표 1–2 | 병합됨 — `o sessions`, `o runs --sessions`, 사무실 세션 보기, 전체 NextUp·추천 | `projects`와 그 아래 링크·정션을 따라가지 않음, `--project`·`--limit`을 세션까지 합친 목록에 적용, 세션 티켓에 O 명령을 내지 않음 |
| `<개인>-pr-review` | 목표 3, `o review` | 병합됨 — `o review`, 앱 검토 요청, `review.posted` | 검토문을 카드가 못 쓰는 O 폴더에 새 파일로 저장, head·base 커밋 기록과 게시 직전 재확인, task 전체 20,000자 배분, 앱의 깨끗한 에이전트 설정 전달, 검토 카드에는 컨텍스트 주입 안 함 |

남은 한계(다음 작업):

- 컨텍스트 C2: FTS 진행을 이벤트 개수로만 판단한다. 같은 개수로 다시 쓰인 실행은 옛 결과가 검색에 남는다(재수집 세대 기록 필요).
- 컨텍스트 C3: 앱·`o runs`의 색인 갱신은 FTS까지 동기화하지 않는다. 같은 실행에서 막 끝난 카드 결과는 다음 dispatch나 `o context`(카드 밖)가 갱신해야 보인다.
- 세션 O2: 같은 크기이거나 더 커진 파일로 통째로 바뀐 transcript는 캐시가 이어 읽기로 판단해 옛 내용을 보일 수 있다.
- 예산 B3: 슬롯을 오래 기다린 다음 카드는 사용량을 다시 조회하지 않고 저장된 예산으로 출발한다.
- 토큰 비교: `o tokens --compare`는 기술 통계다. 예산이 낮춘 실효 effort·성공 여부·같은 작업/커밋을 맞추지 않으므로 그대로 절감률 근거로 쓰지 않는다.

## 토큰 최적화 측정 방법

비교 대상은 A(평소 프로필·기본 컨텍스트, 공유 선택 주입 없음)와 B(깨끗한 프로필 + 필요한 공유 컨텍스트)다.
현재 가능한 `--clean` 비교만으로 B 전체가 구현됐다고 간주하지 않는다. 공유 주입 경로를 통합한 뒤 본 비교를 한다.

기록할 항목:

- 실행 식별: A/B, runId, cardId, 프로젝트 시작 커밋, 동일 계획 파일의 해시, 공식 CLI 버전, provider·model·effort.
- 컨텍스트: 기본 지침 목록/버전, 공유 기록의 고정 시점, 선택한 원본 위치·길이·주입량, clean 점검 결과.
- 카드 토큰: `card.usage`의 `input`, `cached`, `output`을 카드별로 합산하고, 그 합을 실행별·프로바이더별로 모은다.
- 결과: `card.end` 상태, 검증 통과/실패, 사람이 고친 내용·재시도 수, 완료 시간, 한도·쿨다운과 측정 누락.
- 부대 비용: 총괄의 계획 수립, 검색·요약 준비, 실패한 카드와 재작업의 토큰도 별도 기록한다. 카드 합계만으로 전체 비용을 주장하지 않는다.

집계의 근거는 `run/events.rs`, `providers/`, `run/runner.rs`, 양쪽 `snapshot` 구현의 `card.usage`다.
정규화된 `input`은 캐시 입력을 제외한다. Codex는 보고 입력에서 캐시를 빼고, Claude는 캐시 생성 입력을 input에 더한다.
Antigravity는 보고된 thinking 토큰을 output에 더한다. 프로바이더 원시 합계를 다시 더해 이중 집계하지 않는다.
`input + cached`는 전체 입력량, `input + cached + output`은 보고 토큰 합계다. 비용·구독 비율과 같은 수치는 아니다.
usage 이벤트가 없거나 필드가 누락된 카드는 0 사용으로 단정하지 않고 측정 누락으로 별도 표시한다.

공정한 A/B 실행 순서:

1. 홈 폴더 밖의 승인된 테스트 프로젝트와 정답/검증 기준을 준비하고, 동일 계획을 두 번 실행한다.
2. 계획의 카드·작업 문구·provider·model·effort·프로젝트 경로를 고정하고, 매 실행 전 파일을 같은 시작 상태로 복원한다.
3. A는 `o dispatch plan.json`, B는 `o dispatch --clean plan.json`을 바탕으로 실행한다.
   B의 공유 컨텍스트 활성화 방법은 context 브랜치의 실제 인터페이스를 따른다. 두 계획의 본문은 바꾸지 않는다.
4. 공유 기록은 A/B 실행 전의 동일한 시점으로 고정하고, 먼저 실행한 쪽의 답·수정·실험 기록이 다음 실행에 들어가지 않게 한다.
5. `o agents doctor --json`으로 B의 격리를 확인하고, 상위 폴더 지침·Orca 훅의 영향과 A/B 계정의 모델 접근 차이를 기록한다.
6. 같은 동시성·한도 조건에서 실행한다. 목표 4의 자동 effort는 이 실험에서 고정해 문맥 절감과 섞지 않는다.
7. 같은 plan의 A→B 쌍에 더해 B→A 순서로 반복해 캐시·시간 영향을 확인하고, 캐시 적중량을 따로 비교한다.
8. `o events <runId>` 또는 실행의 `events.jsonl`로 집계한다. `o tokens`는 통합 후 이 원본과 대조한 결과만 사용한다.

사전에 합의할 성공 기준:

- 같은 검증 기준을 모두 통과하고 필수 규칙·정확성·결과 범위가 유지될 것; 누락·재작업 증가는 함께 평가할 것.
- 전체 입력량과 비캐시 입력량의 절감률 `1 - B/A`를 각각 보고하고, A가 0인 지표는 비율을 계산하지 않을 것.
- 출력·캐시·부대 비용·완료 시간도 함께 공개하고, 측정 누락이 있는 표본으로 확정적인 절감률을 주장하지 않을 것.
- 허용 재작업/지연, 최소 절감률, 반복 횟수·대상 계획 수는 대표와 팀이 실행 전에 정할 것. 아직 합의된 수치는 없다.

## 작은 다음 단계의 전체 순서

1. 병렬 브랜치의 실제 변경과 ADR 상태를 확인하고, 기록·프로젝트 경계·토큰 집계 계약부터 맞춘다.
2. clean + 공유 주입 + `o tokens`를 한 계획에 연결해 위 A/B를 실행하고 절감 근거를 남긴다.
3. 같은 기록을 사용해 예산/effort를 검증한 뒤, NextUP 재개·PR 검토·부모/자식 시각화를 각각 연결한다.
4. 총괄 설정과 Composer의 실제 전달은 연결됐다(ADR-0008). 카드 질문 응답과 재시도까지 이어 사람이 앱에서 맡기고·확인하고·다시 맡기는 경로를 완성한다.

이 문서 추가의 검증은 경로·구현 근거 확인과 `cargo run --release -p o-cli -- scan --all`이다.
실제 유료 CLI 실행, A/B 측정, 앱 조작, 병렬 브랜치 통합 검증은 이 문서 작업에서 수행하지 않는다.
