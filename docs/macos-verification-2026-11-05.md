# macOS 실기 검사 — 2026-11-05

## 결론

Apple Silicon에서 test_1 앱 생성 및 첫 화면 실행을 확인했다. test_2의 독립 프로세스·자원 검사 11개 중 5개 통과, 6개 실패했다. **앱 패키징 성공을 실제 작업 지원 완료로 해석하면 안 된다.**

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

## 제품 검사 결과

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

## 수정 우선순위와 경계

1. 총괄의 분리된 자손이 살아 있는데 종료로 보이는 문제와 파일 이름 별칭 중복 예약부터 해결해야 한다. 동시에 같은 파일을 수정할 가능성이 있다.
2. 카드 정상 종료/중지 및 총괄 중지 후 상태 복구를 구현한다. `sys/proc.rs`의 macOS `contain()`은 현재 `None`이며 `stop_and_confirm()`도 false다. 이는 확인한 코드 상태이며, 모든 실패의 단일 원인이라고 단정하지 않는다.
3. Windows의 작업 그룹 소유 기능에 해당하는 macOS 종료 증명이 필요하다. PID만 저장해 종료하거나, 예약을 무조건 해제하거나, 실패 검사를 통과로 바꾸는 방식으로 고치지 않았다. 이 PR은 검증 도구 수정과 실패 재현 기록이며 제품의 6개 실패를 해결하지 않는다.

실제 공식 로그인, 유료 AI 요청, 강제 종료 후 재시작 경쟁, Intel 빌드는 미검증이다.
