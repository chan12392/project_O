# glib 취약점과 정기 공급망 점검

- 기준 커밋: `565e8702382ac6afb046b523dde8241d676accaa`
- 작업 브랜치: `younchan-dependency-security`
- 범위: supply F9의 glib 결함·정기 의존성 검사. 원본 6개 감사 보고서와 최종 remediation 집계는 변경하지 않았다.

## 수정

glib 0.18.5의 [GHSA-wrw7-89jp-8q8g](https://github.com/advisories/GHSA-wrw7-89jp-8q8g) / [RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html)에 [공식 두 줄 수정](https://github.com/gtk-rs/gtk-rs-core/commit/b5a4071e439bef2b5eea76c3aa25e5ae84839e34)을 적용했다. `let p` → `let mut p`, C out-argument `&p` → `&mut p`다. 버전 숫자는 0.18.5 그대로이며 `[patch.crates-io]`가 Linux의 GTK 관련 의존성을 이 소스로 연결한다.

2026-11-12 crates.io 조회에서 Tauri 최신 안정은 2.12.5, WebKitGTK는 2.0.2다. 두 경로는 glib/gtk 0.18을 요구한다. 새 gtk 0.19.0은 glib ^0.22를 요구하므로 개별 교체가 호환 업그레이드가 되지 않는다. 공식 glib 0.18 최신 배포는 0.18.5이고 공식 0.18 브랜치에도 결함이 남아 있다. [현재 Tauri manifest](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri/Cargo.toml), [확인한 glib 0.18 소스](https://github.com/gtk-rs/gtk-rs-core/blob/42b9caf98e03ded086362d9653ca58fe94dc8658/glib/src/variant_iter.rs)를 확인했다.

원본 crate의 SHA-256을 Cargo.lock의 기존 checksum과 대조했다. 전체 121개 파일을 비교하여 `src/variant_iter.rs`의 두 줄 외에는 원본과 같은 바이트임을 확인했다. 출처·원본/패치/트리 해시와 원본 MIT LICENSE·COPYRIGHT는 [vendor 기록](../../vendor/README.md)에 있다. 보관량은 약 1.72 MB다.

## 자동 감시

- `npm run check:dependencies`: Python 3.11+ 표준 라이브러리로 루트·앱 npm audit와 OSV 공식 API의 RustSec/GHSA 자료를 조회한다. 추가 감사 도구 설치가 없고, 잠금 파일 전체의 플랫폼·개발 의존성을 검사한다.
- glib도 원래 0.18.5로 계속 조회한다. 검토한 전체 소스 해시와 manifest/lock의 출처가 맞을 때만 두 경보 ID를 `BACKPORTED`로 출력한다. 새 취약점·unsound 경보, 통신 실패·응답 누락·잘못된 페이지·npm 실패는 종료 코드 1이다. 일반 ignore 목록은 없다.
- 기존 필수 `policy`에 넣었다. 첫 도입 이후에는 기준 브랜치의 검사기를 보존해서 PR의 소스를 검사한다. 보호 브랜치·대표의 `.github/` PR 작성 규칙·읽기 전용 토큰·SHA 고정 Actions를 유지했다.
- `.github/workflows/dependency-security.yml`은 월요일 02:23 UTC에 같은 검사와 Linux GLib 일반/최적화 회귀 테스트를 실행한다. 실제 정기 실행은 기본 브랜치에 반영된 뒤 시작된다.
- Dependabot 자동 버전 PR의 기본 브랜치 이름은 팀 브랜치 규칙과 맞지 않아 활성화하지 않았다. 기존 경보는 닫지 않았으며 수정은 개인 작업 브랜치에서 검토한다.

## 확인한 결과

| 검사 | 결과 |
|---|---|
| 루트·앱 `npm audit --json --ignore-scripts` | 모두 종료 코드 0, 알려진 취약점 0 |
| `npm run check:dependencies` 실제 네트워크 조회 | 통과, upstream 잠금 버전 464개, 모든 플랫폼, 두 glib ID는 백포트로 명시 |
| `npm run test:dependencies` | 10개 통과: 패치 변조/추가 파일/원래 registry 출처·Cargo 해석 결과 거부, 페이지 처리, 불완전 응답, npm 오류, 통신 오류와 새 취약점의 실패 전파 |
| 공식 원본 121개 파일 대조 | 변경 파일 1개, 공식 수정 두 줄과 일치 |
| Linux 역방향 의존 트리 | glib 0.18.5가 로컬 패치로 선택됨; gio/gtk/WebKitGTK 관련 경로 확인 |
| Windows x64 역방향 의존 트리 | glib 사용 경로 없음 |
| `cargo check --workspace --locked` (Windows) | 데스크톱 셸 포함 통과 |
| npm 라이선스 검사 | 통과 |
| 두 workflow의 YAML 파싱·읽기 전용 권한·Action SHA·실패 무시 없음 확인 | 통과 |
| 새 Rust fixture의 rustfmt | 통과 |
| `cargo run -p o-cli -- scan --all` | 757개 파일 통과 |
| 기존 `tests/ci-security.test.ts` | 브랜치·PR 작성자 정책 테스트 통과 |
| staged `git diff --check` | 통과. 원본 LICENSE의 마지막 빈 줄만 경로별 whitespace 속성으로 보존 |

실제 조회에서 유지보수 중단 경고는 4건이다. 이들은 별도 `UNMAINTAINED`로 출력하며 취약점 수정으로 집계하지 않는다: proc-macro-error 1.0.4 (`RUSTSEC-2024-0370`), zbus_names 4.3.5 (`RUSTSEC-2026-0371`), zvariant 5.15.1 (`RUSTSEC-2026-0370`), zvariant_derive 5.15.1 (`RUSTSEC-2026-0372`).

## 아직 확인하지 않은 범위

현재 환경에는 사용 가능한 WSL/Linux 실행 수단과 cargo-audit가 없다. 로컬 도구를 설치하지 않았으며, 실제 Linux GLib 회귀 테스트를 실행했다고 하지 않는다. 대신 실제 GLib을 쓰는 `tests/glib-variant/` fixture와 CI 경로를 마련했다. 정상·빈/Unicode 문자열, forward/reverse/교차 순회, nth/nth_back/last, 빈 배열·경계 초과·잘못된 Variant 타입을 일반·opt-level 2 **test** 프로필에서 검증한다. 실행 명령은 vendor 기록에 있다.

원격 workflow 실행·경보 닫기·push·릴리스는 하지 않았다. 코드의 해당 결함은 공식 백포트로 수정했고 로컬에서 가능한 검사는 통과했지만, Linux 바이너리 실행 및 정기 CI의 실제 작동은 통합 후 확인해야 한다. 버전만 보는 Dependabot 경보는 남을 수 있다. 새 백포트에는 소스·검사기 기대 해시의 검토가 필요하며 호환 공식 버전 전환 시 이 보관본을 제거한다. [ADR-0050](../decisions/0050-glib-backport-and-dependency-checks.md)에 결정을 기록했다.
