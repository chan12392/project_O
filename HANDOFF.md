# 인계장

| 갈래 | 한 줄 요약 | 마지막 갱신 | 급함 |
|---|---|---|---|
| B | 맥 6개 재현 오류 수정·전체검사·앱설치 완료, 하위 작업 제한 잔존 | 2026-11-05 | 하위 프로그램 전체 지원은 제한 |
| A | macOS 앱 빌드·실제 시작 검증 완료 | 2026-11-02 21:51 | 사람 검토·공식 CLI 로그인 |

## A. macOS 앱
- 사용자 지시로 macOS 앱 제작. Discord 2026-11-02 21:05 대표가 브랜치 생성 허용한 내용을 사용자와 확인했고, 이후 사용자가 작업 시작 지시.
- 기준 origin/test_2 7fcb86b, 작업 브랜치 eunmi-macos. 보호 브랜치·타인 브랜치 수정 금지.
- 앞선 검사: 이전 07fc05c에서 화면 테스트64개/빌드/브라우저 데모 성공. 실제 앱·AI 실행과 구분.
- Apple Command Line Tools 있음. Homebrew rustup 및 Rust stable 1.99.1 설치 완료. 명령 실행 시 PATH 앞에 /opt/homebrew/opt/rustup/bin 추가.
- 맥 app/dmg 설정·CLI 동봉·Finder CLI 탐색·Terminal 로그인·Windows 업데이트 차단 구현. 맥 프로세스 시작 시각 및 임시 폴더 별칭 비교 보완.
- 현재 기준 화면 검사 65개·타입 검사·화면 빌드·Rust 전체 검사·clippy 통과. Rust 검사는 실제 TMPDIR 경로 사용(첨부파일의 심볼릭 링크 거부 규칙 유지). 정책 검사 344개 통과.
- 2026-11-02 21:51 Apple Silicon app 빌드 성공. 사용자 Applications에 복사해 실제 실행, 첫 화면·설정·정보·Windows 업데이트 차단·3개 CLI 탐색 확인. Codex/Antigravity 로그인 상태 미확인, Claude 분리 프로필 로그인 필요. 실제 로그인·AI 작업·Terminal 로그인 창·Intel·DMG 배포는 아직 미검증.
- 코드 구현 커밋 0e9f9f8, origin/eunmi-macos push 완료. 검토용 draft PR https://github.com/chan12392/project_O/pull/27 생성. GitHub 검사 진행 중. 사람 검토 후 통합하며 보호 브랜치 직접 수정·병합 없음.
- 다음: 사용자 공식 CLI 로그인 후 빈 프로젝트에서 실제 작업 검증. Windows와 동등한 비정상 종료 하위 프로세스 정리는 아직 보장하지 않음. 외부 채팅 메시지 발송 승인 없음.

## B. macOS 오류 수정 — 2026-11-05

- 사용자 요청: 남은6개 오류 수정·맥 실사용 가능하도록 계속 진행.
- 작업 브랜치 eunmi-macos-validation, PR68에 이어서 수정. 보호 브랜치 변경 없음.
- 파일 별칭: CoreFoundation Unicode 정규화·볼륨 대소문자 정책, 기존 예약 키도 비교 때 변환.
- macOS 시작 전 정지한 posix_spawn + kqueue NOTE_FORK/NOTE_EXIT 감시. 직접 자식 정상/중지 증명은 가능; fork가 발생하면 자손 종료 보증이 없어 불명 유지. 실제 CLI 도구 실행 전체 지원 완료 아님. ADR0030 참조.
- 셸 없는 native posix_spawn 최종 실행부에서 맥 회귀14개(제품11+보조3) 통과. 전체 Rust workspace 통과(/tmp/project-o-mac-final-tests.log), 화면100개·타입·clippy·정책 통과.
- 기존 앱 검사2개는 변경된 토큰 응답/대화 잠금 계약을 반영하지 못해 실패. 기대 응답 및 테스트 준비 순서 수정(제품 기능 완화 없음).
- 코드 커밋 ceb8794 push 완료. PR68 제목/본문을 최종 수정으로 갱신, draft 유지·병합 없음. GitHub branch-flow 통과, policy 진행 중(13:20 확인).
- 최종 arm64 앱+o 빌드 성공. /Users/piglet/Applications/project_O.app 교체 설치 및 실제 창·남아 있는 예약 없음 표시 확인. 이전 앱은 /Users/piglet/Applications/Project_O_backups/project_O-before-20261105-fix.app 보존.
- 다음: 실제 공급자 도구 실행에서 fork 이후 종료를 입증할 실행 격리 설계. 현재는 하위 프로그램을 만든 작업이 끝나도 불명으로 남을 수 있으므로 완전한 맥 연속 코딩 지원 완료가 아니다. 실제 로그인·유료 요청·Intel 미검증. 외부 채팅 발송 없음.

<details><summary>이전 맥 실기 기록</summary>
## B. macOS 재검증 — 2026-11-05

- 작업 브랜치 eunmi-macos-validation, 기준 origin/test_2 1fe1412. test_1 b650a88 별도 빌드 성공.
- 현재 결과와 재현: docs/macos-verification-2026-11-05.md. 제품 실패 6개는 미해결.
- 검사 스크립트 setsid 반환값 오류 수정, 은미 브랜치 등록을 AGENTS/guard/CI에 동기화.
- 독립 검사 경로 /private/tmp/o-nov5-qa-su014nue, 항목별 로그 보존. 실제 계정 요청/Discord 발송 없음.
- 이전 A 갈래의 11월 2일 성공 결과는 당시 버전 결과다. 최신 macOS 작업 지원 완료로 해석하지 말 것.
- test_2 표준 macOS 빌드도 성공, 앱+o arm64 확인 및 첫 화면 실행 확인. ~/Applications/Project_O-test2-20261105.app (실사용 완료 아님).
- 수정/실기보고 커밋 9f45a6c, draft PR #68 → test_2: https://github.com/chan12392/project_O/pull/68. 제품 실패 6개 미해결, 병합 없음.

</details>
