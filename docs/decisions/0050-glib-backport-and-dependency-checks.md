# ADR-0050: glib 보안 백포트와 정기 의존성 검사

- 상태: 채택
- 날짜: 2026-11-12
- 범위: 공급망 F9, Linux Tauri 의존 경로, PR·정기 검사

## 배경

glib 0.18.5의 `VariantStrIter`가 C out-pointer를 불변 참조로 전달한다. 최적화 시 NULL 포인터를 읽을 수 있다([RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html)). 수정 배포는 0.20.0부터지만 현재 안정 Tauri/GTK3/WebKitGTK는 glib 0.18에 묶여 있다. Windows에 없는 의존성이라는 이유로 Linux 결함을 남겨 두지 않는다.

## 결정

1. 공식 glib 0.18.5 crate를 출처·해시·MIT 라이선스와 함께 보관하고 [공식 커밋](https://github.com/gtk-rs/gtk-rs-core/commit/b5a4071e439bef2b5eea76c3aa25e5ae84839e34)의 두 줄 수정만 적용한다. Cargo `[patch.crates-io]`로 사용하며 버전 숫자는 바꾸지 않는다. [소스와 유지 절차](../../vendor/README.md)를 둔다.
2. `npm run check:dependencies`는 루트·앱의 npm audit(개발 의존성 포함)와 [OSV 공식 API](https://google.github.io/osv.dev/post-v1-querybatch/)의 RustSec/GHSA 자료를 사용한다. Python 3.11+ 표준 라이브러리만 사용하며 추가 감사 도구 설치는 필요 없다. 두 Cargo.lock의 모든 플랫폼 의존성을 확인한다.
3. glib 전체 소스 해시, 실제 Cargo patch 선택, 두 lock의 버전·로컬 출처를 확인해야 해당 glib 0.18.5의 두 경보 ID만 `BACKPORTED`로 보고한다. glib 자체도 0.18.5로 계속 조회한다. 새 경보·다른 버전·다른 출처·소스 변경은 통과시키지 않는다. 포괄 ignore 목록은 두지 않는다.
4. HTTP 실패·시간 초과·잘못된 응답·누락된 배치·반복 페이지·npm 실패는 종료 코드 1이다. RustSec의 `unmaintained`는 별도 출력하고, 실제 취약점과 `unsound`는 실패한다. 유지보수 중단 경고를 취약점 수정으로 집계하지 않는다.
5. PR의 필수 `policy` job에서 검사한다. 기존 기준 브랜치 검사기 보존 방식을 따른다. 최초 도입 시 기준에 파일이 없을 때만 PR 파일로 시작하고, 이후에는 기준의 검사기를 실행한다. `.github/` PR 작성자·보호 브랜치 규칙은 바꾸지 않는다.
6. 별도 read-only workflow가 매주 월요일 02:23 UTC에 검사한다. 기본 브랜치에 반영된 뒤 일정이 활성화된다. 자동 PR·경보 닫기·릴리스·상태 수정은 하지 않는다. Dependabot 기본 PR 브랜치 이름은 현재 팀 브랜치 규칙과 맞지 않으므로 자동 버전 PR 설정은 추가하지 않는다. 기존 보안 경보는 유지하고 수정은 개인 작업 브랜치에서 검토한다.
7. Linux GLib 라이브러리를 사용하는 별도 fixture를 일반·opt-level 2 test 프로필로 PR·정기 CI에서 실행한다. 실제 Iterator의 정상·경계 동작을 검사하며 제품 코드를 복제하지 않는다.

## 영향과 남은 확인

출처 보관에 약 1.72 MB와 121개 파일이 필요하다. 새 경보는 기존 버전으로 계속 감시하므로 백포트 이후 추가 보안 수정은 직접 검토해야 한다. 호환 공식 경로로 옮길 때 패치·검사기·fixture를 함께 제거한다. 잠금 버전 기반 Dependabot 경보는 자동으로 닫히지 않을 수 있다.

현재 Windows 작업 환경에는 Linux 실행 수단이 없어 Linux 바이너리 실행 결과를 주장하지 않는다. 작성한 CI를 원격 실행하지 않았으며 실행 검증·정기 일정 활성화는 통합 후 남는다. 검증 결과는 [작업 보고서](../reports/2026-11-12-dependency-security.md)에 기록한다.
