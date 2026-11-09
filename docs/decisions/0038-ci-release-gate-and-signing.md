# ADR-0038: CI 전체 검사, Windows 검사, 릴리스 관문, 선택적 코드 서명

- 상태: 채택
- 날짜: 2026-11-10
- 관련: [ADR-0003](0003-team-flow-test1-test2.md), [ADR-0021](0021-test1-hotfix-branch.md), [ADR-0031](0031-test1-approval-and-main-hotfix.md), [설치 안내](../INSTALL.md)

## 배경

코드 검토에서 확인한 세 가지 빈틈이 있다.

1. **검사 건너뛰기.** `guard.yml`은 `test_2`·`test_1_hotfix`·`test_1`·`main_hotfix`에서 올린 PR의 병합 결과가 head와 같으면(`git diff --quiet HEAD^2 HEAD`) o scan·Rust 테스트·clippy·TS 테스트를 건너뛰었다. head가 실제로 통과했는지는 보지 않았다. PR #95(`test_1` → `main`, 실행 42579702356)는 무거운 단계를 모두 건너뛴 채 통과했다. 각각 따로 검사받은 두 PR을 `test_2`에 연달아 병합하면 합쳐진 트리는 한 번도 검사받지 않을 수 있다. 대표는 규칙 집합을 우회할 수 있어 이 구멍을 사람이 메우기 어렵다.
2. **Windows가 CI 밖에 있다.** CI는 ubuntu에서 `--exclude project-o`로 돈다. Tauri 셸은 CI에서 빌드·테스트되지 않는다. job object·프로세스 격리·가짜 프로바이더 실행기 테스트(`crates/o-core/tests/runner.rs` 등) 같은 Windows 전용 코드도 마찬가지다. `release.yml`(windows-latest, `test_1` push)은 NSIS 설치 파일을 빌드해 프리릴리스로 올리기만 한다. 설치한 앱이나 실제 에이전트 작업은 아무것도 돌려 보지 않는다.
3. **서명이 없다.** 설치 파일·`project_O.exe`·`o.exe` 어디에도 코드 서명이 없다(signtool 사용처 없음). 업데이트는 GitHub가 준 SHA-256만 확인한다(`app/src-tauri/src/commands/update.rs`). 이 확인은 받은 파일이 올라간 자산과 같은지만 본다. 누가 만들었는지는 보지 않는다.

## 선택지

1. 건너뛰기를 고쳐서 남긴다(head의 검사 결과를 API로 확인). 검사 결과 조회에 쓰기 권한이 필요 없지만, "같은 트리가 어디선가 통과했다"를 정확히 증명하기 어렵고 우회 경로가 남는다.
2. 건너뛰기를 없애고 모든 PR이 전체 검사를 돈다. CI 시간이 늘지만 판단이 단순하다.
3. 실제 에이전트(Claude·Codex 로그인) 검사를 CI에서 돌린다. 구독 로그인 정보를 CI 비밀로 둬야 한다. 로그인 정보를 PC 밖에 두지 않는다는 보안 원칙(AGENTS.md)과 맞지 않는다.
4. 서명을 필수로 한다. 인증서가 아직 없어 릴리스가 멈춘다.

## 결정

선택지 2를 고르고, 실제 에이전트 검사는 대표 PC의 스크립트로, 서명은 선택 사항으로 둔다.

- **건너뛰기 제거.** `guard.yml`의 모든 PR이 전체 검사(o scan, Rust 테스트·clippy, TS 테스트·타입 검사, 라이선스 검사)를 돈다. 포크 PR 처리(`branch-flow.yml`이 거부), SHA로 고정한 action, 읽기 전용 토큰은 그대로다. `fetch-depth: 2`는 건너뛰기에만 쓰여 뺐다.
- **Windows job.** `guard.yml`에 job `windows`(windows-latest)를 더한다. 화면을 빌드해(`npm run build --prefix app`, Tauri 크레이트가 `app/dist`를 싣는다) `cargo test --workspace --no-fail-fast`와 `cargo clippy --workspace --all-targets -- -D warnings`를 `project-o`까지 포함해 돌린다. 셸은 bash로 둔다. pwsh 래퍼는 마지막 명령의 종료 코드만 보기 때문이다. 기존 Linux job `policy`는 그대로 `project-o`를 뺀다.
  - **규칙 집합.** 보호 브랜치 5개의 규칙 집합은 지금 `policy`·`check`를 요구한다. 대표가 필수 검사에 **`windows`**를 더해야 이 job이 병합을 막는다. 이 ADR은 GitHub 설정을 바꾸지 않는다.
- **릴리스 관문(CI).** `guard.yml`에 `workflow_call`을 더한다. `release.yml`은 job `checks`로 같은 커밋에서 guard 전체(Linux와 Windows)를 다시 돌린다. 설치 파일 job은 `needs: checks`다. 빌드한 뒤 연기 시험을 하고, 하나라도 실패하면 게시하지 않는다.
  - 연기 시험은 `RUNNER_TEMP` 아래에 `/S /D=`로 조용히 설치한다. 설치 파일(`project_O.exe`, `o.exe`, `o-uninstall-data.exe`, `uninstall.exe`, 라이선스 파일)과 제거 레지스트리를 확인한다.
  - 설치된 `o.exe`로 `o about --json`, `o ram --json`, 2카드 예시 계획의 `o check`(정책 검사만 하고 전달하지 않음)를 돌린다.
  - `uninstall.exe /S`로 제거한 뒤 앱 파일과 레지스트리 키가 남지 않았는지 본다.
  - guard의 동시 실행 그룹에 호출한 워크플로 이름을 넣었다. 릴리스가 부른 검사가 PR 검사를 취소하거나 기다리지 않게 하려는 것이다.
- **릴리스 관문(로컬, 실제 에이전트).** 대표가 `main` 릴리스 전에 자기 PC에서 `pwsh scripts/release-gate.ps1`을 돌린다. 구독 사용량을 조금 쓴다. 스크립트는 다음을 차례로 확인하고, 마지막에 PASS/FAIL 표를 보인다. 실패하면 종료 코드 1, 사람 확인을 건너뛰면 2다.
  - 사전 조건: 에이전트 환경이 아닌지, `o about`, `o agents doctor --json`의 Claude·Codex 로그인.
  - 홈 폴더 밖(기본 `<시스템 드라이브>\project_O-release-gate`)에 임시 git 프로젝트를 만든다.
  - `o dispatch`로 claude·codex 2카드 계획을 보내고 두 카드와 실행이 모두 `done`인지 본다.
  - 오래 걸리는 카드를 보내 시작 뒤 `o stop`을 하고 `stopped`인지 본다.
  - `o followup`으로 보낸 후속 실행이 `done`인지 본다.
  - `o resources --json`에 이 실행들의 자원 예약이 남지 않았는지(회수) 본다.
  - 업데이트 경로는 읽기 전용으로 확인한다. 앱 업데이터와 같은 규칙으로 제안될 릴리스를 고르고, 그 커밋이 `test_1`에 있는지 본다. 받은 설치 파일의 크기·SHA-256을 확인하되 설치하지 않는다. 서명 상태는 보여 주기만 한다.
  - 총괄 승인과 계획 "실행"은 사람의 결정이다. `o lead decide`는 에이전트 환경에서 거부된다(ADR-0008). 그래서 스크립트는 이 둘을 자동화하지 않고 사람이 확인할 항목(M1~M3)으로 묻는다.
- **코드 서명(선택).** `scripts/build-installer.ps1`은 서명 설정이 있을 때만 서명한다. Tauri 번들러의 `bundle.windows` 설정을 빌드 때만 임시 설정 파일로 합친다. Tauri가 `project_O.exe`·`o-uninstall-data.exe`·사이드카 `o.exe`·설치 파일·제거 파일에 서명한다. 빌드 뒤 `signtool verify /pa`로 `project_O.exe`, `o.exe`, 설치 파일을 확인하고, 하나라도 실패하면 빌드를 멈춘다.
  - 서명 설정은 다음 셋 중 하나만 둔다.
    - `PROJECT_O_SIGN_PFX_BASE64` + `PROJECT_O_SIGN_PFX_PASSWORD`: PFX를 내보낼 수 없는 키로 `Cert:\CurrentUser\My`에 넣고, 끝나면 저장소에서 뺀다. `release.yml`은 같은 이름의 저장소 비밀을 넘긴다.
    - `PROJECT_O_SIGN_THUMBPRINT`: 이미 저장소에 있는 인증서. 하드웨어 토큰이 여기에 해당한다.
    - `PROJECT_O_SIGN_COMMAND`: Tauri `signCommand`. Azure Trusted Signing 같은 클라우드 서명 도구를 쓸 때다.
  - 타임스탬프는 RFC 3161(`/tr`, 기본 `http://timestamp.digicert.com`)이다.
  - 비밀 값은 출력하지 않는다. PFX 비밀은 가져온 직후 환경변수에서 지운다. 그래서 뒤에 도는 npm·cargo 빌드 스크립트는 이 값을 보지 못한다.
  - 서명 설정이 없으면 "서명하지 않은 빌드" 안내(Actions에서는 경고 표시)를 하고 그대로 계속한다. 릴리스 노트는 설치 파일의 Authenticode 상태에 따라 서명·미서명을 적는다.
  - 서명된 빌드는 연기 시험에서 설치된 `project_O.exe`·`o.exe`의 서명도 확인한다.

## 영향

- 모든 PR의 CI가 길어진다. `test_2` → `test_1` → `main`처럼 이미 본 내용도 다시 돌고, Windows job은 Linux보다 느리다. 대신 병합되는 트리마다 Windows까지 검사받는다.
- Windows job이 새 실패를 드러낼 수 있다. Windows에서만 깨지는 테스트는 이제 PR을 빨갛게 만든다.
- 남은 일:
  - **규칙 집합에 `windows` 필수 검사 추가**(대표, GitHub 설정).
  - **실제 인증서 구입.** 2023년 6월부터 공인 코드 서명 인증서의 키는 하드웨어(토큰·HSM)에 있어야 한다. 그래서 새로 산 인증서는 대개 PFX로 내보낼 수 없다. 사면 `PROJECT_O_SIGN_THUMBPRINT`(로컬 토큰)나 `PROJECT_O_SIGN_COMMAND`(클라우드 서명)를 쓰게 된다. CI에서 클라우드 서명을 하려면 그 도구를 설치하는 단계와 비밀을 `release.yml`에 더해야 한다. PFX 경로는 자체 서명 시험용이나 예전 방식 인증서용이다.
  - 업데이트가 서명을 확인하지 않는다. 지금은 크기·SHA-256만 본다. 서명이 생기면 업데이터가 설치 전에 Authenticode 게시자를 확인하도록 바꾸는 것을 따로 결정한다.
  - 실제 에이전트 관문은 사람이 돌리는 스크립트다. CI가 강제하지 않는다. 대표가 `main` 병합 전에 돌리고 결과를 PR에 적는다.
- 되돌리기: 건너뛰기를 되살리려면 이 ADR을 대체해야 한다. 서명은 비밀을 지우면 바로 미서명 빌드로 돌아간다.
