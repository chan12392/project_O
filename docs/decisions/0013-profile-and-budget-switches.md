# ADR-0013: project_O 프로필 사용과 하루 사용 예산 스위치

- 상태: 채택 (대표 요청)
- 날짜: 2026-11-02
- 관련: ADR-0004(하루 사용 예산), ADR-0005(공유 컨텍스트), ADR-0008(총괄), ADR-0010(대체), ADR-0012(승인 대기 없음)
- 코드: `crates/o-core/src/run/mode.rs`, `crates/o-core/src/usage/budget.rs`, `crates/o-core/src/run/runner.rs`, `policy.rs`(`daily-budget`), 앱 설정 → 에이전트, 입력창 실행 프로필 칩

## 배경

ADR-0010은 깨끗한 프로필, 이전 작업 컨텍스트 주입, 예산에 따른 effort 낮춤을 스위치 하나로 묶었다. 대표가 원한 것은 하루 사용 예산을 켜고 끄는 것이었고, 프로필은 별개의 선택이었다. 그래서 스위치를 둘로 나눈다. 실행 기록(`run.mode`)·정책 규칙(`daily-budget`)·설정 파일이 바뀌므로 ADR로 남긴다(ADR-0012: 기록만, 승인 대기 없음).

## 결정

### 1. project_O 프로필 사용 (프로필만 정한다)

- 켜짐(기본): 카드와 총괄, 앱의 PR 검토를 Project O의 깨끗한 에이전트 프로필(`agents::clean`, 개인 훅·전역 지침·스킬·플러그인·MCP·메모리 없음)로 실행한다.
- 꺼짐: 그 사람의 평소 CLI 설정(스킬·훅·메모리) 그대로 실행한다.
- 공유 컨텍스트 주입(ADR-0005)은 이 스위치와 관계없이 켜져 있다(ADR-0010 이전과 같다). 끄는 길은 CLI의 `--no-context`뿐이다. PR 검토 카드는 지금처럼 컨텍스트를 받지 않는다(ADR-0006).
- CLI: `--profile project-o|own`(`o dispatch`, `o check`, `o lead send`, `o lead dispatch`, `o review`). 주지 않으면 `project-o`. `--clean`은 `--profile project-o`와 같은 뜻으로 남긴다(`--profile own`과 함께 주면 거부). ADR-0010의 스위치 플래그는 `test_2`에만 있었고 릴리스(`test_1`)에 나가지 않아 별칭 없이 뺀다(주면 "알 수 없는 인자").
- 앱 설정은 예전과 같은 `clean` 값(localStorage `project-o.settings`)을 그대로 읽는다. 키를 바꾸지 않으므로 각자 지금 고른 값이 그대로 남는다.
- 입력창 칩은 "project_O 프로필" / "내 설정"으로 보이고, 툴팁이 뜻을 풀어 쓴다.
- 기록: 실행 폴더의 `clean`·`no-context` 표식(`o tokens` 묶음의 근거, 그대로)과 `run.mode { runId, profile, context }` 이벤트. 총괄 기록 `lead.user`·`lead.dispatch`에는 `profile`을 남긴다. ADR-0010 형식의 옛 `run.mode` 줄은 읽을 때 건너뛴다(화면은 원래 이 줄을 쓰지 않고, 묶음은 표식으로 정한다).

### 2. 하루 사용 예산 켜기·끄기

- 켜짐(기본): ADR-0004 그대로. 하루 예산은 주간 한도의 12·13·14%, 하루가 지나는 속도보다 빨리 쓰면 카드 effort를 한두 단계 낮추고, 다 쓰면 그 에이전트의 새 카드를 `daily-budget`으로 막는다.
- 꺼짐: 예산으로 막지도 effort를 낮추지도 않는다. 모든 프로바이더가 `state: "off"`가 되고, 오늘 쓴 양 숫자와 사용량 패널의 "오늘" 고리는 "예산 꺼짐"으로 표시된 채 남는다. 상태 표시줄과 계획 카드는 예산 소진 경고를 띄우지 않는다.
- 바뀌지 않는 것: 구독 자체의 한도(CLI가 알려 주는 한도·쿨다운), `LIMITS`의 동시 실행 상한, 가드 규칙. 날짜별 카드 수 제한은 ADR-0045에서 제거했다.
- 저장: `<O 상태 폴더>/settings.json`의 `dailyBudgetEnabled`. `false`일 때만 꺼짐이고 없거나 잘못된 값은 켜짐이다. 비율(`dailyBudgetPercent`)은 꺼도 지우지 않는다. 설정 화면에서는 비율 선택이 스위치 옆에 있고, 꺼져 있으면 비활성으로 보인다.
- 읽는 곳: `budget::assess_now`가 스위치를 읽어 꺼져 있으면 `Pace::Off`를 돌려준다. 정책 문맥(`policy_context` → `over_budget`), 러너의 카드 시작 직전 확인과 effort 낮춤, `o budget`, 앱·개발 서버의 `/budget`이 모두 이 한 곳을 거치므로 앱·개발 API·CLI가 같은 답을 낸다.
- CLI: `o budget --on|--off`. 앱: Tauri `budget_set_enabled`, 개발 서버 `POST /api/budget/enabled?value=on|off`.
- 끄는 것은 사용을 늘리는 방향이지만 상한(`LIMITS`)이나 구독 한도를 넘게 하지는 않는다. 하루 몫을 나누는 것은 각자의 선택이라 설정으로 둔다.

## 영향

- ADR-0010은 이 ADR로 대체된다.
- `RunMode`는 `{ profile, context }`만 가진다. effort 낮춤은 실행마다 고르지 않고 하루 예산 스위치를 따른다.
- `OEvent::RunMode`의 필드가 바뀌었다. `BudgetReport`에 `enabled`, `Pace`에 `off`가 생겼다(화면은 예전 `o`가 `enabled`를 주지 않으면 켜짐으로 본다).
- 되돌리기: `dailyBudgetEnabled`를 지우면 켜짐으로 돌아간다. 프로필은 앱 설정이나 `--profile`로 언제든 바꾼다.
