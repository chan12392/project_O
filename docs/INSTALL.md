# Windows 설치

project_O 데스크톱 앱(`project_O.exe`)과 `o` CLI(`o.exe`)를 설치 파일 하나로 설치한다. 사용자별 설치라 관리자 권한이 필요 없다.

## 설치 파일 받기

- **GitHub 프리릴리스**: `test_1`에 push될 때마다 `.github/workflows/release.yml`이 같은 커밋에서 전체 검사(Linux·Windows, `guard.yml`)를 다시 돌린다. 이어 설치 파일을 만들고 연기 시험(조용한 설치 → `o.exe` 확인 → 조용한 제거)을 거친다. 모두 통과해야 `test_1-<yyyyMMdd>-<커밋 7자리>` 태그의 프리릴리스로 올린다(ADR-0038). 저장소의 Releases에서 `project_O_<버전>_x64-setup.exe`를 받는다. 같은 파일이 워크플로 실행의 아티팩트(`project_O-setup`)로도 남는다.
- **직접 빌드**: Rust(stable)와 Node 22.18 이상이 있는 Windows에서

  ```powershell
  pwsh scripts/build-installer.ps1
  ```

  마지막 줄에 만들어진 설치 파일 경로(`target\release\bundle\nsis\project_O_<버전>_x64-setup.exe`)가 나온다. 처음 빌드할 때 Tauri CLI가 NSIS 도구를 내려받는다.

## "Windows의 PC 보호" (알 수 없는 게시자)

코드 서명은 선택 사항이다(ADR-0038). 지금 릴리스는 인증서가 없어 **서명하지 않은 빌드**다. 프리릴리스 노트에 "서명되지 않은 테스트 빌드"라고 적힌다. 서명하지 않은 설치 파일을 처음 실행하면 SmartScreen이 "Windows의 PC 보호"와 "알 수 없는 게시자"를 띄운다. 받은 곳(위 프리릴리스 또는 직접 빌드)이 맞는지 확인한 뒤 **추가 정보 → 실행**을 누른다. 서명된 빌드는 노트에 "코드 서명된 테스트 빌드"라고 적히고 게시자가 표시된다. 파일 속성 → 디지털 서명에서도 확인할 수 있다.

### 서명해서 빌드하기 (고치는 사람용)

`scripts/build-installer.ps1`은 아래 중 **하나**가 설정돼 있을 때만 서명한다. 하나도 없으면 "서명하지 않은 빌드입니다 (unsigned)"를 알리고 그대로 빌드한다. 둘 이상이면 멈춘다.

| 환경변수 | 쓰는 경우 |
|---|---|
| `PROJECT_O_SIGN_PFX_BASE64` + `PROJECT_O_SIGN_PFX_PASSWORD` | PFX 파일(base64). 빌드 동안 `Cert:\CurrentUser\My`에 내보낼 수 없는 키로 넣었다가 끝나면 뺀다. `release.yml`은 같은 이름의 저장소 비밀을 쓴다 |
| `PROJECT_O_SIGN_THUMBPRINT` | `Cert:\CurrentUser\My`에 이미 있는 인증서(하드웨어 토큰 등)의 SHA-1 지문 |
| `PROJECT_O_SIGN_COMMAND` | Tauri `bundle.windows.signCommand`(`%1`이 파일 경로). 클라우드 서명 도구용이며, 도구 설치와 그 인증은 쓰는 사람이 맡는다 |

- `PROJECT_O_SIGN_TIMESTAMP_URL`로 RFC 3161 타임스탬프 서버를 바꾼다(기본 `http://timestamp.digicert.com`).
- Tauri가 `project_O.exe`, `o-uninstall-data.exe`, 사이드카 `o.exe`, 설치 파일, 제거 파일에 서명한다.
- 빌드 끝에 `signtool verify /pa`로 `project_O.exe`·`o.exe`·설치 파일을 확인하고, 실패하면 빌드가 실패한다. Windows SDK의 `signtool.exe`가 필요하다.
- 비밀 값은 출력하지 않는다. PFX 비밀은 가져온 직후 환경변수에서 지워 이후 npm·cargo 빌드 스크립트에 넘기지 않는다.
- 2023년 6월 이후 발급되는 공인 코드 서명 인증서는 키가 하드웨어에 있어야 해 PFX로 내보낼 수 없는 경우가 많다. 이때는 지문이나 서명 명령 방식을 쓴다.

## 어디에 무엇이 설치되나

| 폴더 | 내용 | 제거 시 |
|---|---|---|
| `%LOCALAPPDATA%\Programs\project_O` | 앱 폴더: `project_O.exe`, `o.exe`, `uninstall.exe` | 삭제 |
| `%LOCALAPPDATA%\project_O` | O 상태 폴더(실행 기록·`o.db`·설정·하위 첨부, `store::default_home()`) | 데이터 삭제 선택 시만 삭제 |
| `%LOCALAPPDATA%\project_O-agents` | 깨끗한 에이전트 프로필과 그 CLI의 로그인(`clean::default_root()`) | 데이터 삭제 선택 시만 삭제 |
| `%LOCALAPPDATA%\project_O-lead` | 총괄 채팅에 첨부한 이미지·문서 (ADR-0008) | 데이터 삭제 선택 시만 삭제 |
| `%LOCALAPPDATA%\project_O-term` | 터미널 카드의 과제·전달 기록·완료 문서 (ADR-0042) | 데이터 삭제 선택 시만 삭제 |
| `%LOCALAPPDATA%\local.project-o.desktop`, `%APPDATA%\local.project-o.desktop` | 창(WebView2) 데이터·앱 설정 | 데이터 삭제 선택 시만 삭제 |

- Tauri의 사용자별 기본 설치 폴더는 `%LOCALAPPDATA%\<제품 이름>` = `%LOCALAPPDATA%\project_O`로, O 상태 폴더와 겹친다. 그래서 설치 파일이 기본 폴더를 `%LOCALAPPDATA%\Programs\project_O`로 옮긴다(`scripts/installer/hooks.nsh`). 설치 화면에서 상태 폴더를 직접 골라도 같은 곳으로 바꾼다.
- 시작 메뉴에 `project_O`, 바탕 화면에 바로가기가 생긴다(설치 마지막 화면에서 바탕 화면 바로가기를 끌 수 있다).
- 앱과 `o.exe`는 시작할 때 자기 폴더를 그 프로세스의 PATH 맨 앞에 붙인다(저장된 PATH는 바꾸지 않는다). 그래서 앱이나 예약 작업의 `o.exe`가 띄우는 에이전트 CLI·`o dispatch`·카드의 `o msg`/`o context`는 `cargo install` 없이도 함께 설치된 `o.exe`를 쓴다. 터미널에서 `o`를 쓰려면 저장소에서 `cargo install --path crates/o-cli`를 하거나 앱 폴더를 사용자 PATH에 더한다.
- 조용한 설치: `project_O_<버전>_x64-setup.exe /S`. 폴더를 바꾸려면 `/D=<폴더>`를 맨 끝에 따옴표 없이 붙인다.

## 처음 실행

1. 시작 메뉴나 바탕 화면의 `project_O`를 연다.
2. 첫 실행 안내(이후에는 설정 화면)에서 Claude Code·Codex·Antigravity의 설치·로그인 상태를 본다. 설치는 공식 npm 패키지가 있고 npm이 PATH에 있을 때만 앱에서 실행하고, 그 밖에는 공식 설치 안내를 연다.
3. 로그인 버튼은 새 콘솔 창에서 각 CLI의 공식 로그인 명령을 띄운다. 로그인 정보는 각 CLI가 관리하고 O는 읽지 않는다.

## 제거

설정 → 앱 → 설치된 앱 → `project_O` → 제거. 제거 창의 **앱 데이터 삭제**를 선택하면 위 데이터 폴더도 지운다. 선택하지 않으면 데이터는 남는다. 진행 중인 작업을 마친 뒤 제거한다. 설치 폴더의 `project_O.exe`와 `o.exe`가 사용 중이면 종료를 확인하며, 조용한 제거(`/S`)는 해당 파일을 사용하는 프로세스를 종료한다. 다른 폴더의 동명 CLI는 대상이 아니다.

PowerShell 한 줄로도 제거할 수 있다. `o`가 PATH에 없어도 된다. 레지스트리에 저장된 인용부호를 제거한 뒤 `Start-Process -Wait`로 실행하여 NSIS의 임시 제거 프로세스까지 기다린다. 두 명령은 **설정 → 정보**에서 복사할 수도 있다.

데이터 보존:

```powershell
Start-Process -FilePath ((Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\project_O').UninstallString.Trim('"')) -ArgumentList '/S' -WindowStyle Hidden -Wait
```

데이터까지 삭제(되돌릴 수 없음):

```powershell
Start-Process -FilePath ((Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\project_O').UninstallString.Trim('"')) -ArgumentList '/S /PURGE' -WindowStyle Hidden -Wait
```

`/PURGE`는 설치 위치 기억 키와 `project_O mirror sync (test|test_1|test_2|main)`이라는 정확한 이름의 예약 작업도 제거한다. 거울 git 저장소는 남긴다. 평소 CLI 폴더(`~/.claude`, `~/.codex`, `~/.gemini`, `~/.agents`), 프로젝트와 저장소, 공유 WebView2 런타임에는 손대지 않는다. O 상태 폴더 밖의 별도 루트 `project_O-lead`(총괄 첨부·계획 파일)와 `project_O-term`(터미널 카드의 과제·완료 파일)도 함께 지운다. 데이터 루트가 비었거나 링크·junction이면 중단한다. 하위 링크는 대상까지 따라가지 않는다. 예약 작업은 위 이름으로 하나씩 확인하므로 PC의 다른 예약 작업이 손상돼 있어도 멈추지 않는다. 사용 중이라 지우지 못한 폴더는 건너뛰고 나머지를 지운 뒤, 남은 폴더를 함께 알려 준다. 제거 중 오류가 나면 앱은 남아 있어 다시 시도할 수 있지만, 앞서 지운 데이터는 복구되지 않는다.

## 업데이트 알림

릴리스 빌드는 커밋 SHA와 UTC 빌드 시각을 포함하며 설정 → 정보에 표시한다. 앱 시작 10초 뒤와 이후 6시간마다 사용자 PC의 `gh`로 고정 저장소 `chan12392/project_O`의 최근 릴리스 20개를 확인한다. `test_1-<yyyyMMdd>-<소문자 sha7>` 프리릴리스 중 릴리스 워크플로(`github-actions[bot]`)가 태그의 커밋으로 올렸고, GitHub가 SHA-256 digest를 준 유효한 `project_O_<숫자.숫자.숫자>_x64-setup.exe`가 있는 가장 최근 게시본을 고른다. 그 커밋이 `test_1`에 포함됐는지(`gh api …/compare/test_1...<sha>`가 identical·behind) 확인하지 못하면 제안하지 않는다. **커밋이 다르고 게시 시각이 현재 빌드보다 나중일 때** 알림을 띄운다. 로컬 빌드는 자동 확인하지 않고 정보 화면에서만 확인한다.

`gh`가 없거나 로그인·권한·네트워크 문제면 자동 확인은 팝업 없이 끝나고, **설정 → 정보 → 앱 업데이트**에 gh 필요 안내와 마지막 자동 확인 시각·실패 이유가 남는다(`update_status`). 수동 **업데이트 확인**은 [릴리스 다운로드 페이지](https://github.com/chan12392/project_O/releases)와 `gh auth login --hostname github.com` 안내를 보여 준다. 저장소가 공개라 릴리스 페이지에서는 로그인 없이 설치 파일을 직접 받을 수 있다. 앱 안의 자동 확인과 **지금 업데이트**는 지금처럼 로그인한 `gh`가 있어야 한다. 개발 서버와 데모는 데스크톱 앱에서만 확인 가능하다고 안내한다.

**지금 업데이트**를 눌러야 다운로드한다. `gh release download`가 새 임시 폴더에 받은 설치 파일의 이름·크기와 릴리스 자산의 SHA-256 digest를 검증한 후 대화형 설치기를 열고 앱을 종료한다. 설치 완료 화면에서 앱을 다시 열 수 있다. 기존 설치 위치와 데이터는 유지된다. 다운로드 중 나중에/Esc를 누르면 다운로드(`gh`)를 바로 멈추고 설치하지 않는다. 다운로드 중에 다시 누른 "지금 업데이트"는 앞선 취소를 되돌리지 않는다. 오류는 팝업에 표시하고 재시도할 수 있다. **이 버전 건너뛰기**는 이 PC의 앱 설정에 태그를 기억하며, 수동 확인은 건너뛴 버전도 보여 준다.

다운로드는 셸 없이 고정 인자로 `gh`를 호출한다. O가 인증 파일·토큰을 읽거나 전달하거나 사용자 이메일을 전송하지 않는다. 업데이터는 코드 서명을 확인하지 않는다(서명은 선택 사항이고 지금 릴리스는 미서명, ADR-0038). 크기·digest 검증은 받은 파일이 GitHub에 올라간 자산과 같은지만 확인할 뿐, 서명 검증을 대신하지 않는다. 실행한 설치 파일은 `%TEMP%\project_O-update-*`에 남고 Windows 임시 파일 정리 대상이 된다.

## 제거 검증 (개발자)

`pwsh scripts/test-installer.ps1 -Setup <빌드한 setup.exe>`는 앱을 열지 않고 임시 경로에 설치·제거하며 실제 데이터는 건드리지 않는다. 기존 설치가 등록돼 있으면 중단한다. `/PURGE /DATADRYRUN`은 실제 데이터 대신 `%TEMP%\project_O-purge-plan-*\plan.json`에 대상만 기록하고 앱을 제거한다.

통합 시험의 `/PURGE /TESTDATA=<32자리 hex ID>`는 `%LOCALAPPDATA%\project_O-uninstall-tests\<ID>\Local` 및 `Roaming` 아래의 동일한 고정 이름만 삭제한다. 이 모드에서는 실제 예약 작업·설치 위치 기억 키를 삭제하지 않는다. 임의 경로나 상대 경로를 받지 않으며 일반 사용자 명령에는 넣지 않는다. 데이터 도우미는 NSIS와 함께 배포되는 `o-uninstall-data.exe`이고 단독 실행은 데이터 삭제이므로 직접 실행하지 않는다.

## 릴리스 관문 (대표)

`main` 릴리스 전에 대표 PC에서 실제 로그인된 Claude Code·Codex로 `pwsh scripts/release-gate.ps1`을 돌린다(구독 사용량을 조금 쓴다, ADR-0038). 홈 폴더 밖(기본 `<시스템 드라이브>\project_O-release-gate`)에 임시 git 프로젝트를 만들고 다음을 확인한다.

- 2카드 전달(claude·codex) `done`
- `o stop` → `stopped`
- `o followup` → `done`
- 자원 예약 회수(`o resources --json`)
- 업데이트 경로(읽기 전용: 제안될 릴리스, `test_1` 포함 여부, 받은 파일의 크기·SHA-256, 설치하지 않음)

총괄 승인·계획 "실행"·업데이트 화면은 사람이 확인하는 항목으로 묻는다. 결과는 PASS/FAIL 표와 종료 코드(0 통과, 1 실패, 2 사람 확인 미완료)로 나온다. 기본 대상은 설치된 앱의 `o.exe`이고, `-O <o.exe>`로 바꾼다.

## 설치 파일 구성 (고치는 사람용)

| 항목 | 값 |
|---|---|
| Tauri 설정 | `app/src-tauri/tauri.conf.json`의 `bundle`(NSIS만, `installMode: currentUser`, 한국어, 게시자·저작권 `Project O`) + 설치 빌드에서만 합치는 `scripts/installer/tauri.installer.json`(`externalBin: binaries/o`, 사이드카 준비) |
| 사이드카 | `scripts/prepare-sidecar.mjs`(`npm --prefix app run sidecar`)가 release `o.exe`를 `app/src-tauri/binaries/o-<triple>.exe`로 복사(gitignore). `tauri dev`와 `--no-bundle` 빌드는 대신 같은 출력 폴더의 CLI를 빌드한다. |
| NSIS 훅 | `scripts/installer/hooks.nsh` — 기본 폴더를 상태 폴더 밖으로 옮긴다 |
| 설치 폴더 | `%LOCALAPPDATA%\Programs\project_O` (`/D=`로 바꾼 경우 그 폴더) |
| 제거 레지스트리 | `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\project_O` — `UninstallString` = `"<설치 폴더>\uninstall.exe"`, `InstallLocation`, `DisplayVersion` |
| 설치 위치 기억 | `HKCU\Software\Project O\project_O`의 기본값. 다음 설치가 이 폴더를 쓴다. 일반 제거는 남기고 "앱 데이터 삭제"를 고를 때만 지운다 |
| 상태 폴더 | `%LOCALAPPDATA%\project_O`(`store::default_home()`), `%LOCALAPPDATA%\project_O-agents`(깨끗한 에이전트), `%LOCALAPPDATA%\project_O-lead`(총괄 첨부·계획), `%LOCALAPPDATA%\project_O-term`(터미널 카드), `%LOCALAPPDATA%\local.project-o.desktop`(WebView2, 식별자 이름) |
| 릴리스 | `.github/workflows/release.yml`: `test_1` push·`test_1`에서의 수동 실행(다른 ref에서 실행하면 job이 건너뛰어 아무것도 게시하지 않음) → `guard.yml` 전체 검사 → 빌드(서명 비밀이 있으면 서명) → 설치 연기 시험 → 태그 `test_1-<yyyyMMdd(UTC)>-<sha7>` 프리릴리스, 첨부 `project_O_<앱 버전>_x64-setup.exe`, 아티팩트 `project_O-setup`. 같은 날 같은 커밋을 다시 돌리면 파일만 바꾼다 |
| 버전 | 앱 버전은 `app/src-tauri/Cargo.toml`의 `version`(현재 0.3.0)과 `app/package.json`. 크레이트는 워크스페이스 버전을 따른다 |

## 문제 알리기

저장소 Issues에 올리거나 팀 대화에 남긴다. 다음을 함께 적는다.

- 설치 파일 이름(버전)과 받은 곳(프리릴리스 태그 또는 직접 빌드한 커밋)
- Windows 버전, 무엇을 하다가 어떤 화면·메시지가 나왔는지
- `o about --json` 결과(앱 폴더의 `o.exe`로 실행: `& "$env:LOCALAPPDATA\Programs\project_O\o.exe" about --json`)

로그나 상태 폴더 파일을 올릴 때는 계정 이름·이메일·토큰이 없는지 먼저 확인한다.
