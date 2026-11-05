# macOS 실기 검사 — 2026-11-05

## 최신 결과: 6개 오류 수정 후

2026-11-05 재검사에서 기존 실패 6개가 모두 통과했다. 독립 검사 11개와 보조 항목 3개를 Cargo 회귀 검사로 등록했고 14개가 모두 통과했다. Rust 전체 workspace 검사, 화면 로직 100개, 앱 TypeScript 검사, clippy 경고 0개 기준 검사를 통과했다.

- 파일 경로: 볼륨의 대소문자 정책과 Unicode 정규화를 적용한다. 새 예약과 기존 버전의 예약 모두 같은 기준으로 비교한다.
- 작업 시작: macOS native posix_spawn으로 실행 전 정지한 자식을 만든 뒤 종료/분기 감시를 등록하고 시작한다. 인수·환경·작업 폴더를 셸 없이 전달한다.
- 직접 작업: 정상 종료와 중지 시 실제 종료를 확인하고 예약 및 총괄 표식을 해제한다. 시작 직전 중지와 인수·환경 보존도 별도 검사했다.
- 분리된 하위 작업: 살아 있을 수 있으면 불명 상태를 유지하고 다음 작업을 차단한다.

**제한:** 이것은 맥 전체 기능 지원 완료가 아니다. macOS에서 하위 프로그램을 만든 작업은, 그 프로그램이 끝났더라도 현재 추적만으로 전체 종료를 증명할 수 없어 불명 상태로 남는다. 따라서 실제 AI 도구의 파일 편집·명령 실행을 포함한 연속 코딩 작업에는 제한이 있다. 실제 계정 로그인·과금 요청과 Intel은 미검증이다. 상세 설계와 범위는 [ADR-0030](decisions/0030-macos-owned-worker-observation.md)에 기록했다.

아래 표는 **수정 전** 재현 기록이다. 정상 종료/중지와 별칭 비교의 기존 실패는 수정했으며, 종료 증명을 약화해 검사를 통과시키지 않았다.

## 환경과 범위

- macOS 26.6.2 (25G83), arm64. Intel 미검증.
- test_1: b650a88. `cargo install --path crates/o-cli --locked` 성공. `npm run tauri build -- --bundles app` 성공, 첫 연결 화면·설치된 CLI 표시 확인.
- test_2: 1fe1412. 별도 작업 브랜치에서 검사. 실제 계정 로그인/과금 작업 없이 독립 임시 패키지와 가짜 자식 프로세스만 사용했다.
- 팀 요청: 일반 채널 2026-11-03의 test_1 빌드 / test_2 버그 PR, 윤찬 작업 채널 2026-11-04의 실제 맥 미검증 항목.
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
