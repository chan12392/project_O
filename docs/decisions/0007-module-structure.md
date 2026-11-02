# ADR-0007: 모듈 구조와 확장 지점

- 상태: 승인 (2026-11-02 — 대표 요청으로 구조 정리와 함께 작성)
- 날짜: 2026-11-02
- 승인: [x] A  [ ] B (사후 확인 필요)
- 관련: [docs/ARCHITECTURE.md](../ARCHITECTURE.md), ADR-0001(스택), ADR-0002(카드 권한), ADR-0005(세션 공유)

## 배경

코어·CLI·셸·화면이 각각 한 덩어리로 커졌다. `crates/o-core/src`는 평평한 모듈 21개였고, 프로바이더마다 다른 동작(실행 인자, 스트림 해석, 설치·로그인, 깨끗한 프로필, 사용량 조회)이 `providers.rs`·`agents.rs`·`probe.rs`·`clean.rs` 네 곳의 `match ProviderId`에 흩어져 있었다. `o` 명령은 `main.rs` 한 파일(768줄)에 도움말 문자열과 분기가 따로 있었고, Tauri 명령은 `commands.rs` 하나, 화면은 `components/` 폴더 하나와 551줄 `Settings.tsx`였다.

다음 작업(총괄 채팅: CLI별 헤드리스 프롬프트·세션 재개·첨부)과 이후 기능을 더할 때마다 여러 파일의 분기를 함께 고쳐야 했다. 빠뜨리면 보안 동작(카드 권한, 깨끗한 환경)이 프로바이더마다 어긋날 수 있다.

## 선택지

1. 지금 구조를 유지하고 문서로만 안내한다 — 변경 지점이 그대로 흩어져 있다.
2. 크레이트를 더 나눈다(`o-providers`, `o-store` 등) — 경계는 뚜렷하지만 순환 의존을 풀어야 하고 이번 범위보다 크다.
3. 크레이트는 그대로 두고 크레이트 안을 도메인 폴더로 묶은 뒤, 구현이 여럿인 곳만 등록 표(트레이트·배열)로 바꾼다. 동작은 바꾸지 않는다.

## 결정

3을 고른다. 파일은 `git mv`로 옮겨 이력을 잇는다.

- **o-core**: `run/`(plan·events·runner·store·index·snapshot·team), `providers/`, `agents/`(상태·설치·로그인, `clean`), `usage/`(probe·budget), `context/`, `sys/`(proc·resolve·js·mirror). `policy.rs`는 공용 경계라 맨 위에 둔다. `review.rs`, `sessions.rs`는 그대로다. 다시 내보내기(re-export)는 하지 않고 경로를 고쳤다.
- **프로바이더 등록부**: `providers/mod.rs`의 `Provider` 트레이트와 `providers::get(ProviderId)`. CLI마다 한 파일(`claude.rs`, `codex.rs`, `agy.rs`)이 실행 인자·권한, 프롬프트 안내, 스트림 해석, 공식 설치 정보, 로그인 명령과 응답 읽기, 깨끗한 프로필의 환경변수·준비·오염 검사, 사용량 조회를 가진다. `ProviderId`는 그대로 직렬화 id다. 총괄 채팅 메서드가 들어갈 자리를 트레이트에 표시만 해 두었다.
- **o-cli**: `commands/<명령>.rs`와 `COMMANDS` 표(이름·도움말 줄·처리 함수). `o help`는 표에서 만들어지며 출력은 이전과 같다.
- **Tauri 셸**: `commands/<도메인>.rs`와 `commands::handler()` 한 곳의 등록. `main.rs`는 그 함수만 부른다.
- **화면**: `shell/`, `features/{chat,office,work,usage,settings}`, `data/`, `platform/backend/{types,tauri,dev,demo}`, `lib/`, `ui/`. 구현이 셋 이상인 곳만 목록으로 만든다: 설정 항목(`sections/index.ts`), 오른쪽 패널 보기(`PANEL_VIEWS` + `SidePanel`의 `VIEWS`), 작업 탭 하위 보기(`SUBS`), `Backend` 구현 셋. 모두 평범한 배열·객체다.

## 확장 지점

| 더할 것 | 새 파일 | 등록 |
|---|---|---|
| 에이전트 CLI | `providers/<cli>.rs` | `ProviderId` 변형 + `providers::get` 한 줄 (+ `LIMITS`는 ADR) |
| `o` 하위 명령 | `o-cli/src/commands/<이름>.rs` | `mod` + `COMMANDS` 한 줄 |
| Tauri 명령 | `src-tauri/src/commands/<도메인>.rs` | `handler()` 한 줄, 개발 서버 `/api` 한 줄, `Backend` 메서드 |
| 패널 보기 / 작업 탭 하위 보기 / 설정 항목 | 기능 폴더의 파일 | `PANEL_VIEWS`+`VIEWS` / `SUBS` / `sections/index.ts` 한 줄 |

절차는 ARCHITECTURE.md "새 기능 추가하기"에 있다. 실행 중에 제3자 코드를 불러오는 플러그인은 두지 않는다. 카드 권한·깨끗한 환경·셸 없는 실행 같은 보안 경계를 지킬 권한 모델이 먼저 필요하므로 별도 ADR로 다룬다.

## 영향

- 동작·출력·화면은 바뀌지 않는다. 기존 테스트(`cargo test --workspace`, `npm test`)와 `o --help` 출력 비교로 확인했다. 프로바이더 보안 동작은 `tests/{providers,clean,agents}.rs`가 그대로 고정한다.
- Rust 경로가 바뀌었다(예: `o_core::plan` → `o_core::run::plan`, `o_core::clean` → `o_core::agents::clean`, `o_core::probe` → `o_core::usage::probe`). 진행 중인 다른 브랜치는 병합할 때 `use` 경로를 고쳐야 한다. 사용량 파서는 `providers::{codex,agy}`로, 짧은 명령 실행(`capture`)은 `sys::proc`로 옮겼다.
- 문서·주석의 파일 경로를 새 위치로 고쳤다(AGENTS.md, README, GOALS.md, ADR-0004·0005, 스크립트 주석).
- 되돌리기: 이동과 분리만 했으므로 커밋 단위로 되돌릴 수 있다.
