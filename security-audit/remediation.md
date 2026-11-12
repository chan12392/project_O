# 보안 감사 조치표

2026-11-12. 기준 커밋 `4b607252`, 보안 작업 브랜치 `younchan-security-fixes`.
원본 6개 보고서는 부모 작업 폴더에 그대로 보존하며 이 커밋에 복제하거나 포함하지 않는다. 보고서의 ID를 기준으로 현재 코드·음성/양성 회귀 검증과 대조했다. 공유 경계 결정은 [ADR-0046](../docs/decisions/0046-security-remediation.md)을 따른다.

`fixed`는 아래에 명시한 입력 경로를 수정했다는 뜻이다. `refuted`는 현재 코드에서 주장이 성립하지 않는 경우, `documented-limit`는 남아 있는 신뢰 한계, `deferred`는 필요한 구조적 수정이 아직 안 된 경우다. 부분 보완이 있는 미해결 항목을 완료로 세지 않는다.

## guard.md

| ID | 판정 | 조치·증거·남은 조건 |
|---|---|---|
| G1 | fixed | `o-guard/hook.rs::gh_write_reasons`: 저장소 보호·ref·내용·워크플로 REST 쓰기, GraphQL mutation/불투명 본문, workflow 실행·재실행을 거부한다. 대표에게도 직접 쓰기는 허용하지 않는다. 가드 회귀 테스트의 쓰기 거부와 읽기·PR 생성 허용을 함께 확인했다. 임의 프로그램 내부의 요청까지 해석하지는 않는다(G4). |
| G2 | fixed | 이스케이프/분할 인용을 정리해 검사한다. git/gh 별칭·알 수 없는 외부 명령, push 경로 설정 변경·환경 설정 주입, 목적지 없는 push를 거부한다. 정상 명시적 개인 브랜치 push는 유지한다. 일반 인터프리터 안의 동작까지 보장하지 않는다(G4). |
| G3 | fixed | 경로 정규화에서 verbatim·UNC 접두를 해석한다. 거울 대상 Write와 상태 경로 회귀 테스트가 거부를 확인한다. |
| G4 | documented-limit | 단순 분할 인용·이스케이프는 보완했다. 동적 변수·와일드카드·인코딩·별도 스크립트 전체를 정규식으로 해석할 수 없다. 파일/네트워크 권한의 OS 경계가 필요하며 우회 불가능하다고 주장하지 않는다. |
| G5 | fixed | 새 도구라도 `command`/`cmd` 인자가 있으면 셸 검사를 한다. 알 수 없는 도구의 보호 브랜치 push 거부를 검증했다. |
| G6 | fixed | 실제 git push 서브커맨드일 때만 목적지 패턴을 적용한다. 커밋 메시지에 push와 main이 있어도 허용한다. |
| G7 | documented-limit | 조회 뒤 원격 PR base가 바뀌는 경합은 남는다. 서버의 보호 규칙이 최종 경계다. 별도 gh 호출 전후 재조회로 원자성을 보장했다고 하지 않는다. |
| G8 | fixed | 가드 프로브는 표준 설치 위치의 절대 Git/gh 경로를 사용한다. PATH와 현재 폴더의 가짜 프로그램은 선택하지 않는다. 동일 사용자의 설치 파일·표준 위치 환경 조작까지 인증하지는 않는다. |
| G9 | fixed | push refspec의 원격 목적지도 개인/작업 브랜치 규칙으로 검사한다. 임의 최상위 브랜치 생성은 거부하고 기존 태그 흐름은 유지한다. |
| G10 | fixed | `o hook`이 CLI의 작업 개체 판정을 입력에 덮어써 전달한다. 호출자가 가짜 판정 값을 주어도 실제 프로세스 판정을 대체하지 못한다. |
| G11 | fixed | 사람 전용 자원 복구 서브커맨드 자체를 차단해 확인 옵션 앞 개행에 의존하지 않는다. CLI 게이트도 유지한다. |
| G12 | refuted | 보고서 자체가 취약점 미발견으로 분류한 양성 통제다. 카드 필드·ID·의존성·자원 경로의 기존 정책을 약화하지 않았다. |

## execsec.md

| ID | 판정 | 조치·증거·남은 조건 |
|---|---|---|
| F1 | fixed | `providers/claude.rs`: 모든 인증 방식의 카드에서 프로젝트 설정을 제외하고 O 가드와 엄격한 MCP 구성을 적용한다. 가드는 호스트 옆 O의 절대 경로와 별도 인자로 실행해 PATH/셸 대체를 막는다. 기본/YOLO 실행 인자, 실제 가짜 카드 실행을 검증했다. 사용자 프로필 자체는 선택한 신뢰 경계다. |
| F2 | fixed | `providers/codex.rs`, `runner/card.rs`, `lead/mod.rs`: 기본 모드의 카드와 총괄은 위험/미지정 프로젝트 설정을 CLI 시작 전에 거부한다. 빈 내용과 ADR에 명시한 협업 불리언 3개만 엄격한 구문으로 허용하고 링크는 거부한다. 사용자 홈 설정은 프로젝트 설정으로 오인하지 않는다. MCP 설정에서 프로브/CLI 미시작, 정상 협업 설정과 훅 파일만 있는 저장소에서 훅 비활성화 및 카드 완료, 잘못된 값·중복·hardlink·junction 거부를 검증했다. 시작 전 검사와 CLI 읽기 사이의 동일 사용자 경합은 남는다. |
| F3 | refuted | 현재 Antigravity는 `terminal_card()` 경로로 실행된다. 보고서의 headless launch 설정은 현재 카드 실행 경로가 아니다. 실제 터미널 파일 경계의 미해결 문제는 statefs F4/F8에 별도로 남긴다. |
| F4 | documented-limit | Windows 작업 개체를 훅에도 연결했지만 다른 서비스 경유와 비-Windows 환경 표식 제거를 OS 신원으로 막지는 못한다. 표식 검사를 격리라고 부르지 않는다. |
| F5 | deferred | 원시 출력/진단 로그의 민감한 내용과 같은 사용자의 읽기 가능성은 남는다. 모든 문자열을 마스킹하면 정상 결과·검토 증거가 손상되고 비밀 제거도 보장하지 못한다. 저장 권한 분리·보관/내보내기 정책과 함께 해결해야 한다. |
| F6 | fixed | 표식/작업 개체로 식별된 에이전트의 전체 실행·이벤트 조회 CLI를 차단했다. CLI 음성 테스트 통과. 파일을 직접 읽는 동일 사용자 프로세스까지 실행 간 읽기 격리가 생긴 것은 아니다. |
| F7 | refuted | POSIX 환경 변수 이름은 대소문자를 구별한다. 소문자 이름은 공식 CLI가 읽는 대문자 인증 변수와 다르므로 이것만으로 구독 모드 우회가 되지 않는다. Windows는 기존 대소문자 무시 비교를 유지한다. |
| F8 | fixed | `run/team.rs::Received::line`에서 모든 제어 문자를 치환한다. CR, ESC, OSC 종료 문자의 출력 금지와 저장 원문 보존을 함께 시험했다. |
| F9 | documented-limit | 준비 기록은 서명된 보안 증명이 아니다. 동일 사용자에 의한 위조·실행 실패 가능성이 남는다. 실제 Codex 샌드박스 실행 옵션을 해제하지 않으며, 파일 해시만으로 인증됐다고 하지 않는다. |

## tauri.md

| ID | 판정 | 조치·증거·남은 조건 |
|---|---|---|
| F1 | fixed | 업데이터의 gh를 표준 설치 절대 경로로 제한했다. 기존 digest/빌드 증명 검증은 그대로 유지한다. 설치 바이너리 자체를 같은 사용자가 바꾸는 한계는 ADR에 명시했다. |
| F2 | fixed | 앱의 O CLI는 현재 실행 파일 옆 사이드카만 사용하고 PATH 폴백을 없앴다. 기존 미리보기 준비 스크립트가 같은 위치에 빌드한다. |
| F3 | documented-limit | 개발 API 변경 요청에 브라우저 출처 증거를 요구하도록 보완했다. 헤더 없는 POST·다른 Origin 거부 및 같은 Origin 허용 테스트 통과. 로컬 프로세스가 헤더를 위조할 수 있어 인증이 추가됐다고 보지 않는다. |
| F4 | documented-limit | 감사에서 실행 가능한 XSS 경로는 발견되지 않았다. 웹뷰 스크립트가 장악되면 자체 명령을 호출할 수 있다. 프런트가 제공하는 클릭 시각은 같은 스크립트가 위조할 수 있으므로 새 보안 장벽으로 추가하지 않았다. |
| F5 | refuted | 현재 URL은 컴파일된 공식 카탈로그 상수이며 Tauri 새 창 처리도 비-HTTPS를 거부한다. 현 코드에서 비신뢰 URL 주입 경로는 없다. 향후 동적 URL을 받으면 그 경계에서 검증해야 한다. |
| F6 | refuted | 현재 인자는 고정 프로그램에 별도 argv로 전달되고 명령 셸에서 보간되지 않는다. 저장소·query 값으로 셸 명령 주입이 성립한다는 근거가 없다. |

## statefs.md

| ID | 판정 | 조치·증거·남은 조건 |
|---|---|---|
| F1 | deferred | 살아 있는 턴·토큰·프로젝트 결합, 카드의 승인 서버 시작 금지, 미리 심은/링크된 결정 파일 거부를 추가했다. 정상 승인·거부·중지·시간 초과도 검증했다. 요청 후 일반 결정 파일을 같은 사용자가 직접 쓰는 위조는 남는다. 상태 소유권과 인증 IPC를 분리해야 완료된다. |
| F2 | deferred | 상태·이벤트·DB의 직접 파일 위조는 그대로 가능하다. 가드의 상태 경로 차단은 심층 방어이며 기록 출처 인증이 아니다. 별도 OS 권한의 상태 브로커가 필요하다. |
| F3 | deferred | dispatch와 러너 사이 모드 표식 교체를 불변 실행 스냅샷으로 결합하지 못했다. 계획/프로필/모드를 함께 전달하는 실행 시작 API와 상태 권한 분리가 필요하다. 기존 파일 옆에 해시를 더하는 방식으로 해결했다고 하지 않는다. |
| F4 | deferred | 터미널 파일 드롭 프로토콜을 직접 조작하는 동일 사용자 프로세스를 차단하지 못한다. 사람 전용 CLI 게이트는 유지되지만 파일 입력/출력 출처를 인증하는 IPC로 바꿔야 한다. |
| F5 | fixed | 거울 Git의 전역/시스템/환경 주입 설정을 제외하고 저장소의 재작성·외부 명령 설정을 거부한다. 공식 origin만 받고 서브모듈을 따라가지 않는다. 실제 임시 저장소에서 설정 거부를 시험했다. 보고서의 URL 조회가 재작성을 전혀 적용하지 않는다는 설명은 잘못됐지만 저장소 설정 위험은 실제다. 동일 사용자 설정 경합은 격리하지 못한다. |
| F6 | deferred | 승인 뒤 실행 계획 파일의 바꿔치기 창이 남는다. 정책 재검사로 유효성은 보지만 승인 내용과의 동일성은 보장하지 않는다. F3와 함께 실행 시작 시 불변 스냅샷 결합이 필요하다. |
| F7 | documented-limit | 크기/수정 시각 안정성은 동일 사용자 변경을 인증하지 못한다. 다만 현재 실제로 읽은 계획 내용은 화면에 제시되고 자동 실행되지 않으며, 승인 내용은 대화 기록에서 선택한다. 안정성 검사만 우회하면 승인 없이 실행된다는 결론은 성립하지 않는다. 상위 디렉터리 경합은 남는다. |
| F8 | documented-limit | 완료 표식은 전달 파일뿐 아니라 과제에도 있어 비밀이 아니다. 한 복사본만 숨겨 형제 프로세스 위조를 해결했다고 하지 않는다. 터미널 채널의 OS 권한/IPC 분리가 필요하다. |
| F9 | fixed | 거울 동기화·새 대화·첨부 게이트 및 작업 개체를 반영하는 훅 판정을 보완했다. 카드의 승인 서버 시작도 거부한다. CLI 음성 테스트를 추가했다. |
| F10 | fixed | 유효 실행 ID와 일반 디렉터리만 인덱싱하며 링크/junction/hardlink 이벤트는 받지 않는다. Windows 실제 junction·hardlink 음성 테스트 및 정상 증분 인덱싱 통과. 상위 경로를 검사 뒤 바꾸는 경합과 정상 파일 내용 위조는 F2의 한계다. |
| F11 | fixed | verbatim/UNC 정규화와 존재하는 상위 경로의 canonical 경로 비교로 별칭을 확인한다. 경로 별칭을 문자열로만 검사하던 공백을 보완했다. 검사 후 경로 변경까지 막는 OS 격리는 아니다. |
| F12 | fixed | 팀 명령의 인자가 채워진 뒤 공통 `speaker` 경계에서 총괄이 다른 카드 이름을 쓰는 것을 거부한다. 실제 CLI의 사칭 거부를 검증했다. |
| F13 | documented-limit | 동일 사용자 상태 RW를 제거하지 않았다. Windows 사용자 프로필의 일반 권한과 에이전트 격리는 다르며, Unix의 임의 umask 아래에서 다른 사용자 읽기 금지를 보장하지도 않는다. 후속 상태 소유권/ACL 설계가 필요하다. |

## supply.md

| ID | 판정 | 조치·증거·남은 조건 |
|---|---|---|
| F1 | fixed | 공식 GitHub 호스트·대표 owner·저장소 이름을 모두 확인한다. HTTPS/SCP형 SSH/SSH URL 양성과 다른 owner/host/로컬 경로 음성을 시험했다. 예약 작업 설치 스크립트에도 같은 조건을 둔다. |
| F2 | fixed | base에서 빌드한 정책 바이너리와 보존한 라이선스 검사기로 head를 검사한다. head의 Rust/npm 코드를 실행하기 전에 두 검사를 마친다. 워크플로 문법과 검사 순서를 로컬에서 확인했다. 원격 Actions 실행은 하지 않았다. |
| F3 | fixed | 포크 PR의 두 코드 실행 job을 건너뛴다. base에서 실행되는 branch-flow는 포크를 계속 거부한다. 원격 실행 권한을 바꾸지 않았다. |
| F4 | fixed | 브랜치 소유자와 PR 작성자를 연결했다. 대표 작성 예외는 기존 워크플로 변경 규칙과 일관되게 유지했다. 미정 계정은 대표만 허용한다. 실제 워크플로 셸을 실행해 소유자·타인·대표·포크 사례를 검증했다. 원격 직접 push 권한 검사는 아니다. |
| F5 | fixed | merge-guard의 오래된 주석을 AGENTS의 규칙집합 서술과 일치시켰다. 사후 경보임을 유지하며 실제 원격 설정을 조사/변경했다고 주장하지 않는다. |
| F6 | documented-limit | 승인된 배포 브랜치 코드와 릴리스 환경은 서명 체인의 신뢰 대상이다. 악성 코드가 그 브랜치에 이미 승인돼 들어온 위협을 빌드 증명만으로 해소할 수 없다. 서명·릴리스 설정은 변경하지 않았다. |
| F7 | documented-limit | 같은 사용자 권한의 설치 O 바이너리를 같은 사용자의 예약 작업이 실행한다. 그 파일을 바꿀 수 있는 주체와의 격리·권한 상승 방어를 제공하는 설치 구조가 아니다. 설치된 작업/바이너리는 손대지 않았다. |
| F8 | documented-limit | 증명 없는 새 릴리스가 업데이트 후보를 가려 설치 실패를 유발할 가능성은 남는다. 설치 전 증명 검증은 유지돼 임의 설치 파일 실행으로 이어지지는 않는다. 후보 검색/게시자 권한 정책 개선은 별도 작업이다. |
| F9 | deferred | 툴체인 업데이트·취약점 정기 감시는 운영 후속 작업이다. 추가로 Dependabot #1의 `glib 0.18.5` 중간 등급 경보 [GHSA-wrw7-89jp-8q8g](https://github.com/advisories/GHSA-wrw7-89jp-8q8g)를 확인했다. 현재 Linux GTK 경로에 포함되며 Windows x64 의존 트리에는 없다. 수정 버전은 0.20.0이지만 현재 gio·gtk·webkit2gtk가 0.18 계열을 요구하므로 해당 의존 경로의 호환 수정이 필요하다. 경보를 닫거나 패치됐다고 하지 않는다. npm의 알려진 취약점 0개를 전체 Rust/배포 체인 감사 결과로 확대하지 않는다. |
| F10 | refuted | 제거 훅의 테스트 데이터 인자는 제한된 경로와 인자 규칙을 거치며 실제 일반 설치 사용자 경로의 코드 실행 근거가 없다. 테스트 설치/제거를 실행하지 않았다. |

## summary.md의 묶음 대응

- A1→statefs F1, A2→F2, A3→F3, A4→F4, A5→execsec F3, A6→statefs F8, A7→statefs F6/F7, A8→statefs F9, A9→statefs F10.
- B1→execsec F1, B2→F2, B3→F4, B4→F5.
- C1→supply F1/statefs F5, C2→tauri F1, C3→tauri F2, C4→supply F2, C5→supply F3, C6→supply F4, C7→guard G1, C8→guard G2/G3/G4.

## 검증과 제외 범위

- Rust: core 라이브러리, 가드 라이브러리/통합, 프로바이더, 승인 IPC, 인덱스, 팀 메시지, 가짜 총괄/카드 실행기, CLI 경계/총괄 테스트. 수정 후 관련 실패를 재검증했고 합계 401개 통과, 기존 1개 무시. Tauri 셸의 36개도 통과했다. 임시 Windows junction/hardlink도 실제 생성해 거부를 확인했다.
- TypeScript: 개발 API 5개, 실제 branch-flow 셸을 사용하는 정책 테스트 1개 통과. 루트 타입 검사와 앱 타입 검사/빌드 통과. 기존 번들 크기·동적 import 경고는 보안 변경 대상이 아니다.
- 설치된 Claude Code 2.1.302의 실행 코드에서도 exec 훅이 실행 파일과 인자 배열을 따로 넘기고 Windows 자식 창을 숨기는 것을 확인했다. 실제 모델/훅 세션은 시작하지 않았다.
- CI YAML 4개 문법 검사 통과. 실제 원격 workflow, 보호 설정, 릴리스·서명·설치·예약 작업은 실행/변경하지 않았다. 실계정·실제 앱 상태·유료 모델 호출과 공개 공격 재현은 하지 않았다.
- `cargo clippy -p o-core -p o-cli -p o-guard -p project-o --all-targets -- -D warnings` 통과. Unix 전용 동작은 이 Windows 환경에서 실행하지 않았다. `cargo run -p o-cli -- scan --all`은 새 문서를 포함해 602개 파일 통과.

실행한 주요 명령(관련 수정 뒤에는 실패/영향받은 대상만 다시 실행):

```text
cargo test -p o-core -p o-guard -p o-cli --lib --test guard --test providers --test index --test message_integrity --test lead_approvals --test runner --test lead --test agent_boundary --no-fail-fast
cargo test -p o-core -p o-cli -p o-guard --test lead --test runner --test index --test agent_boundary --test guard --no-fail-fast
cargo test -p o-core --test index junction_run_folders_are_not_ingested
cargo test -p o-core -p o-cli -p o-guard --test runner --test providers --test agent_boundary --test guard --no-fail-fast
cargo test -p o-core --test runner
cargo test -p o-core --test providers --test runner --test lead --no-fail-fast
cargo test -p project-o
node --test tests/devserver.test.ts
node --test tests/ci-security.test.ts
npm run check
npm --prefix app run build
```

문서/코드의 충돌도 AGENTS의 Codex 시작 조건·Claude 적용 대상·포크 CI 설명과 merge-guard 주석에서 바로잡았다. 구 ADR의 역사적 설명은 ADR-0046에서 변경 범위를 지정했다. 같은 사용자 상태 위조에 대한 보장은 아직 성립하지 않음을 SECURITY에도 명시했다.
