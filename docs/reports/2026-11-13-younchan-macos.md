# 윤찬 최신 브랜치 macOS 검증 — 2026-11-13

## 기준

- 사용자 요청: `test_2`가 아닌 윤찬 최신 브랜치로 맥 검증과 필요한 수정.
- 원본: `younchan` `6493c1caf7d8a369da2843178d48bed520b23005`. 2026-11-13 19:40 KST GitHub 원본 재확인.
- 작업: `eunmi-latest-macos`. 윤찬 브랜치와 보호 브랜치를 직접 변경하지 않았다. PR125는 이전 `test_2` 기준을 유지한다.
- 플랫폼: 이 Apple Silicon 맥. Intel·Windows·Linux 실기는 포함하지 않는다.

## 수정

1. `9227d1c`: 동시 예약 시험의 임시 폴더 이름에 고유 순번 추가. PR125의 수정만 가져왔다. Wiki 오류는 윤찬 최신본에 이미 수정돼 중복 적용하지 않았다.
2. `96c618f`: Unix에서 실제 경로와 아직 존재하지 않는 경로의 선행 슬래시 표현을 맞춰 보호 범위 판정이 일치하도록 수정. Windows 네트워크 경로 시험의 맥 실패를 재현했고, Unix 이중 슬래시·심볼릭 링크와 인접 폴더 구분도 검증했다. 보호 규칙을 완화하지 않는다.
3. `96c618f`: 실제 맥 앱에서 Windows 제거 명령과 업데이트 안내 노출을 발견. 운영체제를 앱에서 전달해 macOS 교체·Finder 제거 안내를 보여 준다. Windows 설치파일의 자동 조회·수동 조회·설치 시작도 비 Windows에서 차단한다.
4. `887ef6e`: Windows 샌드박스 시험이 맥에서 영구 대기 자식 프로세스를 남기던 시험 코드 수정. 자식 종료 검증은 Windows에서 유지하고 다른 플랫폼에서는 시간 초과 검증만 수행한다. 해당 시험에서 남았던 프로세스만 신원을 대조해 종료했다. 실제 Codex 작업의 종료 실패와 구분한다.

## 실제 AI·네이티브 앱

원본 기준 release 앱 빌드 성공 후 별도 상태 폴더에서 실제 창을 열었다. 개인 공식 Codex 로그인, `own` 프로필, `normal` 권한, 원본 작업은 컨텍스트 주입 없이 작은 읽기 전용 과제다. 전역 설정·로그인 파일·기존 설치 앱을 변경하지 않았다.

- 원본 프로젝트: `/Users/piglet/Projects/project-o-latest-macos-smoke-20261113`. numbers.txt 내용 19와 23.
- 실행 `20261113-103727-d6yn`: `MAC_LATEST_OK:42`, card.end done/exit0, run.end done.
- 앱 입력창에서 후속 지시 `20261113-103753-akvc`: `MAC_FOLLOWUP_OK:84`, card.end done/exit0, run.end done.
- 두 실행은 같은 Codex 세션을 이어갔다. 앱에서 두 결과와 완료 상태를 직접 확인했다.
- 실제 모델은 앱에 gpt-6-astra로 표시. 모델을 강제로 바꾸지 않았다. 사용량은 일부 확인으로 표시됐으며 완전한 정산으로 주장하지 않는다.
- 종료 뒤 `o resources --json`: `{"leases":[]}`.
- 임시 상태: `/private/tmp/o-younchan-native-20261113/project_O`. 원본 숫자 파일은 보존했다.

## 자동 검사

- TypeScript 264개, 루트 타입, 앱 타입·화면 빌드 통과.
- 경로 보호 시험 35개, 예약 시험 10개 통과.
- 초기 CLI 시험 13개는 현재 소스와 다른 응답을 반환했다. 최신 CLI를 다시 빌드한 뒤 관련 시험 37개 모두 통과. 초기 산출물 불일치를 제품 수정으로 집계하지 않는다.
- 새 자동 전환 시험: CLI/총괄/백그라운드 진입점 11개, 카드 7개·총괄 2개 시나리오 통과. 실제 구독 한도를 소진시켜 전환하지 않았다.
- 최종 전체 Rust workspace 종료 코드0: 표준 시험629 통과·실패0·제외8 및 별도 실행 방식 검사 통과. 제외 항목은 Windows 전용4, 실제 Codex 수동2, 성능1, 설치CLI수동1이다.
- 전체 실행 후 시험 자식 프로세스 정리 수정은 해당 sandbox 시험5개를 다시 실행해 통과. 남은 sandbox 시험 프로세스0 확인. 최종 clippy workspace/all-targets 경고 없음.
- npm runtime 라이선스 검사 통과.
- 수정본 release 앱 빌드 성공. `/Users/piglet/Applications/Project_O_latest_20261113.app`에 별도 배치했고 기존 `/Users/piglet/Applications/project_O.app`은 교체하지 않았다.
- 수정 앱 정보 화면에서 커밋96c618f, 별도 상태 경로, macOS 교체·Finder 제거 안내를 실제 확인. Windows 명령·업데이트 버튼은 표시되지 않는다. 재실행 후 앞선 두 완료 작업도 복원됐다.
- 설치 앱의 제품 코드는96c618f다. 후속887ef6e는 시험 코드만 바꿨으며 제품 실행파일 차이는 없다.

## 재현 명령

```sh
TMPDIR=/private/tmp cargo build -p o-cli
TMPDIR=/private/tmp cargo test --workspace --no-fail-fast
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p o-cli -- scan --all
npm test
npm run check
npm run build --prefix app
npm run check:licenses
npm run tauri --prefix app -- build --bundles app
```

맥의 `/var`와 `/private/var` 별칭으로 시험 경로가 달라지지 않게 실제 임시 경로를 지정한다. 링크 보호를 해제하지 않는다.

## 범위와 후속

실제 Codex 읽기 작업·같은 세션 후속·종료 후 예약 해제와 앱 첫 화면·정보 화면을 검증했다. 실제 작업 중 사용자 중지, 파일 수정 작업, 다른 AI 공급자, 자동 구독 전환 실기, Intel Mac은 별도다. 일반적인 모든 맥 작업이나 모든 보안 문제의 해결을 주장하지 않는다. 최신 윤찬 브랜치 전체를 PR125에 섞지 않았으며 이 수정의 비교 기준은 원본6493c1c다. 외부 채팅 발송과 보호 브랜치 병합은 하지 않았다.
