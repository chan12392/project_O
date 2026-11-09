# macOS 실기 검사 — 2026-11-05

## 최신 결과: 하위 프로그램 종료·실제 AI 작업

작업별 launchd resource coalition을 도입했다. 부모에서 분리되거나 출력을 붙잡는 하위 프로그램까지 종료하고, 커널의 활성 작업 수로 끝났음을 확인한다. 단순히 fork가 있었다는 이유로 다음 작업을 막던 제한을 해소했다. [ADR-0030](decisions/0030-macos-owned-worker-observation.md).

실제 ChatGPT 로그인 상태의 공식 Codex CLI 0.166.0으로, 별도 시험 폴더와 별도 O 상태 폴더에서 기본 모드(sandbox 유지)를 검증했다. API 키·인증 파일을 읽거나 복사하지 않았다.

| 실제 작업 | 결과 | 근거 |
|---|---|---|
| Python 파일 작성·실행 | 완료 | `python3 smoke.py` → `MAC_O_SMOKE_OK`, 종료 코드 0, result.txt 생성 |
| 같은 작업의 후속 지시 | 완료 | 같은 세션에서 재실행, result.txt에 `FOLLOWUP_OK`, 상태 done |
| AI 명령 실행 중 중지 | 중지 완료 | 같은 coalition의 프로그램 7개 관찰, 0.11초 뒤 동일 신원 잔존 0개, 상태 stopped |
| 작업 예약 해제 | 통과 | 정상·후속·중지 뒤 `o resources --json` → `{"leases":[]}` |

첫 실제 시도는 설치된 Codex 0.150.1이 현재 모델을 지원하지 않아 실패했다. 공식 npm의 0.166.0으로 갱신한 뒤 위 세 검증을 수행했다. 처음 실패한 기록은 별도 시험 상태 폴더에 보존했고 강제로 예약을 지우지 않았다.

자동 검사는 분리 자손의 PID+시작 시각을 검사해 실제 종료 후에만 예약이 풀리는지 확인한다. 별도 검사로 이웃 프로그램/동시 작업 보존, 소유자 강제 종료, 시작 전 중지, 가득 찬 출력 파이프, 정확한 환경·인수·cwd, 잘못된 PID 세대 신호 거부를 검증한다. 기존 deadline 검사는 macOS가 고아 출력을 조기에 정리하는 경우 실제 자손 종료·오답 없음·다음 턴 가능까지 확인하도록 변경했다. 시간 초과·중지 검사는 유지한다.

최종 검사: Rust 전체 workspace 통과, 화면100개 통과, 앱 TypeScript·clippy 경고0·정책471파일 통과. macOS 앱(62.29 MiB)과 동봉 o 모두 arm64 빌드 성공. `~/Applications/project_O.app`에 교체 설치하고 기존 앱은 백업했다. 전역 o도 동봉 버전과 동일한 SHA-256으로 갱신했다.

설치된 release 도우미로 실제 fork/명령을 실행해 `RELEASE_HELPER_OK`, 종료 상태0, 커널 시작8/종료7(도우미만1) 및 종료 증명 true를 확인했다. 실제 앱에서 Codex 0.166.0 연결됨, 개인 CLI 프로필·Codex 선택, 초기 연결 완료, 남아 있는 예약 없음까지 확인했다. YOLO는 끄고 하루 예산은 켠 상태를 유지했다.

빌드 코드: 95c99f3(구현), 650807b(검증 기록·검사). origin/test_2=1fe1412 기준, PR68에 반영하며 병합하지 않는다.

범위: 이 Apple Silicon 맥(macOS 26.6.2)과 Codex에서 검증. Intel·다른 macOS 버전·다른 AI 서비스의 실제 호출은 미검증. 필요한 커널 기능/GUI launchd 도메인을 확인할 수 없으면 안전하게 시작을 거부한다. 도우미 자체의 강제 종료 등 종료 증명이 사라진 경우는 기존처럼 불명 기록을 보존한다.

<details><summary>앞선 수정과 최초 재현 기록</summary>


## 최신 결과: 6개 오류 수정 후

2026-11-05 재검사에서 기존 실패 6개가 모두 통과했다. 독립 검사 11개와 보조 항목 3개를 Cargo 회귀 검사로 등록했고 14개가 모두 통과했다. Rust 전체 workspace 검사, 화면 로직 100개, 앱 TypeScript 검사, clippy 경고 0개 기준 검사를 통과했다.

- 파일 경로: 볼륨의 대소문자 정책과 Unicode 정규화를 적용한다. 새 예약과 기존 버전의 예약 모두 같은 기준으로 비교한다.
- 작업 시작: macOS native posix_spawn으로 실행 전 정지한 자식을 만든 뒤 종료/분기 감시를 등록하고 시작한다. 인수·환경·작업 폴더를 셸 없이 전달한다.
- 직접 작업: 정상 종료와 중지 시 실제 종료를 확인하고 예약 및 총괄 표식을 해제한다. 시작 직전 중지와 인수·환경 보존도 별도 검사했다.
- 분리된 하위 작업: 살아 있을 수 있으면 불명 상태를 유지하고 다음 작업을 차단한다.

**제한:** 이것은 맥 전체 기능 지원 완료가 아니다. macOS에서 하위 프로그램을 만든 작업은, 그 프로그램이 끝났더라도 현재 추적만으로 전체 종료를 증명할 수 없어 불명 상태로 남는다. 따라서 실제 AI 도구의 파일 편집·명령 실행을 포함한 연속 코딩 작업에는 제한이 있다. 실제 계정 로그인·과금 요청과 Intel은 미검증이다. 상세 설계와 범위는 [ADR-0030](decisions/0030-macos-owned-worker-observation.md)에 기록했다.

수정본 .app과 동반 o가 모두 arm64인 것을 확인하고 `~/Applications/project_O.app`에 설치했다. 실제 창에서 연결 화면과 “남아 있는 예약이 없습니다” 표시를 확인했다. 기존 앱은 별도 백업했다.

아래 표는 **수정 전** 재현 기록이다. 정상 종료/중지와 별칭 비교의 기존 실패는 수정했으며, 종료 증명을 약화해 검사를 통과시키지 않았다.

## 환경과 범위

- macOS 26.6.2 (25G83), arm64. Intel 미검증.
- test_1: b650a88. `cargo install --path crates/o-cli --locked` 성공. `npm run tauri build -- --bundles app` 성공, 첫 연결 화면·설치된 CLI 표시 확인.
- test_2: 1fe1412. 별도 작업 브랜치에서 검사. 실제 계정 로그인/과금 작업 없이 독립 임시 패키지와 가짜 자식 프로세스만 사용했다.
- 팀 요청: 일반 채널 2026-11-03의 test_1 빌드 / test_2 버그 PR, 2026-11-04 팀 작업 채널의 실제 맥 미검증 항목.
- 기존 PR #27은 다른 기준 코드다. 그 PR 전체를 그대로 병합하는 대신 현재 test_2에서 재현한 결과를 기록한다.

## 최신 앱 빌드

`npm run tauri -- build --config src-tauri/tauri.macos.conf.json` 성공(62.14 MiB). 앱과 동반 `o` 실행 파일이 모두 arm64임을 `file`로 확인했다. 앱 첫 연결 화면, 설치된 CLI 목록, 자원 예약 점검 버튼을 실제 창에서 확인했다. 로그인 버튼과 실제 작업 실행은 검증하지 않았다.

## 검사 도구 수정

`scripts/macos-process-probe.py`가 세션 번호를 읽을 때 `ValueError: invalid literal for int() with base 10: 'None'`으로 중단됐다. `os.setsid()`는 반환값을 세션 번호로 사용할 수 없으므로, 호출 뒤 `os.getsid(0)`으로 읽도록 수정했다.
수정 후 직접 자식 정상 종료, 분리된 자손의 stdout 유지/닫힘 두 경우 모두 통과했다. 검사 볼륨에서 대소문자 및 조합형/완성형 이름은 같은 파일로 관찰됐다.

## 수정 전 제품 검사 결과

`docs/macos-local-verification.md` 절차에 따라 독립 Cargo 패키지를 만들고 `--offline -- --list`로 11개 검사와 보조 3개를 확인했다. 아래 검사를 각각 `--exact --nocapture`로 실행했다.

| 검사 | 결과 | 관찰 |
|---|---|---|
| macos_case_alias_reservation | 실패 | 없는 하위 경로의 CaseName/casename에 중복 예약 허용 |
| macos_unicode_alias_reservation | 실패 | 없는 하위 경로의 é/e+결합악센트에 중복 예약 허용 |
| macos_ambiguous_spawn_recovery | 통과 | 시작 여부 불명 예약 보존 |
| macos_unstarted_owner_recovery | 통과 | 시작 전 소유자 종료 확인 후 복구 |
| macos_card_normal | 실패 | Done을 기대했으나 Failed |
| macos_card_stop | 실패 | 중지 후 신속 종료 실패, 약 5초 후 가짜 자식 자체 종료 |
| macos_card_escaped_closed | 통과 | 분리된 자손이 stdout을 닫아도 예약 보존 |
| macos_card_escaped_pipe | 통과 | 분리된 자손이 stdout 유지 시 예약 보존 |
| macos_lead_normal | 통과 | 정상 종료 후 다음 턴 가능 |
| macos_lead_stop | 실패 | 중지 후 다음 턴 차단 상태 잔존 |
| macos_lead_escaped_descendant | 실패 | 자손 생존 중 busy=unknown 기대, 실제 null |

## 수정 전 진단과 경계

1. 총괄의 분리된 자손이 살아 있는데 종료로 보이는 문제와 파일 이름 별칭 중복 예약부터 해결해야 한다. 동시에 같은 파일을 수정할 가능성이 있다.
2. 카드 정상 종료/중지 및 총괄 중지 후 상태 복구를 구현한다. `sys/proc.rs`의 macOS `contain()`은 현재 `None`이며 `stop_and_confirm()`도 false다. 이는 확인한 코드 상태이며, 모든 실패의 단일 원인이라고 단정하지 않는다.
3. Windows의 작업 그룹 소유 기능에 해당하는 macOS 종료 증명이 필요하다. PID만 저장해 종료하거나, 예약을 무조건 해제하거나, 실패 검사를 통과로 바꾸는 방식으로 고치지 않았다. 이 문단은 수정 전 진단이다. 후속 수정은 위 최신 결과와 ADR-0030을 따른다.

실제 공식 로그인, 유료 AI 요청, 강제 종료 후 재시작 경쟁, Intel 빌드는 미검증이다.

## 검사 기반 보수

전체 검사 과정에서 기존 앱 토큰 응답 검사와 오래된 대화의 실행 여부 검사 준비 순서를 현재 제품 계약에 맞췄다. macOS 회복 검사와 독립 검사 임시 폴더 이름에는 원자적 순번을 더해 동시 검사 시 시각이 같아 폴더가 겹치는 문제를 막았다. 실행 제한·정책 차단·종료 불명 보존을 해제하지 않았다.

</details>
