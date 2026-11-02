# 인계장

| 갈래 | 한 줄 요약 | 마지막 갱신 | 급함 |
|---|---|---|---|
| A | macOS 앱 빌드·실제 시작 검증 완료 | 2026-11-02 21:51 | 사람 검토·공식 CLI 로그인 |

## A. macOS 앱
- 사용자 지시로 macOS 앱 제작. Discord 2026-11-02 21:05 대표가 브랜치 생성 허용한 내용을 사용자와 확인했고, 이후 사용자가 작업 시작 지시.
- 기준 origin/test_2 7fcb86b, 작업 브랜치 eunmi-macos. 보호 브랜치·타인 브랜치 수정 금지.
- 앞선 검사: 이전 07fc05c에서 화면 테스트64개/빌드/브라우저 데모 성공. 실제 앱·AI 실행과 구분.
- Apple Command Line Tools 있음. Homebrew rustup 및 Rust stable 1.99.1 설치 완료. 명령 실행 시 PATH 앞에 /opt/homebrew/opt/rustup/bin 추가.
- 맥 app/dmg 설정·CLI 동봉·Finder CLI 탐색·Terminal 로그인·Windows 업데이트 차단 구현. 맥 프로세스 시작 시각 및 임시 폴더 별칭 비교 보완.
- 현재 기준 화면 검사 65개·타입 검사·화면 빌드·Rust 전체 검사·clippy 통과. Rust 검사는 실제 TMPDIR 경로 사용(첨부파일의 심볼릭 링크 거부 규칙 유지). 정책 검사 344개 통과.
- 2026-11-02 21:51 Apple Silicon app 빌드 성공. 사용자 Applications에 복사해 실제 실행, 첫 화면·설정·정보·Windows 업데이트 차단·3개 CLI 탐색 확인. Codex/Antigravity 로그인 상태 미확인, Claude 분리 프로필 로그인 필요. 실제 로그인·AI 작업·Terminal 로그인 창·Intel·DMG 배포는 아직 미검증.
- 코드 구현 커밋 0e9f9f8, origin/eunmi-macos push 완료. 검토용 draft PR 준비. 사람 검토 후 통합하며 보호 브랜치 직접 수정·병합 없음.
- 다음: 사용자 공식 CLI 로그인 후 빈 프로젝트에서 실제 작업 검증. Windows와 동등한 비정상 종료 하위 프로세스 정리는 아직 보장하지 않음. 외부 채팅 메시지 발송 승인 없음.
