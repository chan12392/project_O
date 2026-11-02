# Windows 설치

project_O 데스크톱 앱(`project_O.exe`)과 `o` CLI(`o.exe`)를 설치 파일 하나로 설치한다. 사용자별 설치라 관리자 권한이 필요 없다.

## 설치 파일 받기

- **GitHub 프리릴리스**: `test_1`에 push될 때마다 `.github/workflows/release.yml`이 설치 파일을 만들어 `test_1-<yyyyMMdd>-<커밋 7자리>` 태그의 프리릴리스로 올린다. 저장소의 Releases에서 `project_O_<버전>_x64-setup.exe`를 받는다. 같은 파일이 워크플로 실행의 아티팩트(`project_O-setup`)로도 남는다.
- **직접 빌드**: Rust(stable)와 Node 22.18 이상이 있는 Windows에서

  ```powershell
  pwsh scripts/build-installer.ps1
  ```

  마지막 줄에 만들어진 설치 파일 경로(`target\release\bundle\nsis\project_O_<버전>_x64-setup.exe`)가 나온다. 처음 빌드할 때 Tauri CLI가 NSIS 도구를 내려받는다.

## "Windows의 PC 보호" (알 수 없는 게시자)

설치 파일은 코드 서명을 하지 않았다. 처음 실행하면 SmartScreen이 "Windows의 PC 보호"와 "알 수 없는 게시자"를 띄운다. 받은 곳(위 프리릴리스 또는 직접 빌드)이 맞는지 확인한 뒤 **추가 정보 → 실행**을 누른다.

## 어디에 무엇이 설치되나

| 폴더 | 내용 | 제거 시 |
|---|---|---|
| `%LOCALAPPDATA%\Programs\project_O` | 앱 폴더: `project_O.exe`, `o.exe`, `uninstall.exe` | 삭제 |
| `%LOCALAPPDATA%\project_O` | O 상태 폴더(실행 기록·`o.db`·설정·하위 첨부, `store::default_home()`) | 데이터 삭제 선택 시만 삭제 |
| `%LOCALAPPDATA%\project_O-agents` | 깨끗한 에이전트 프로필과 그 CLI의 로그인(`clean::default_root()`) | 데이터 삭제 선택 시만 삭제 |
| `%LOCALAPPDATA%\project_O-lead` | 총괄 채팅에 첨부한 이미지·문서 (ADR-0008) | 데이터 삭제 선택 시만 삭제 |
| `%LOCALAPPDATA%\local.project-o.desktop`, `%APPDATA%\local.project-o.desktop` | 창(WebView2) 데이터·앱 설정 | 데이터 삭제 선택 시만 삭제 |

- Tauri의 사용자별 기본 설치 폴더는 `%LOCALAPPDATA%\<제품 이름>` = `%LOCALAPPDATA%\project_O`로, O 상태 폴더와 겹친다. 그래서 설치 파일이 기본 폴더를 `%LOCALAPPDATA%\Programs\project_O`로 옮긴다(`scripts/installer/hooks.nsh`). 설치 화면에서 상태 폴더를 직접 골라도 같은 곳으로 바꾼다.
- 시작 메뉴에 `project_O`, 바탕 화면에 바로가기가 생긴다(설치 마지막 화면에서 바탕 화면 바로가기를 끌 수 있다).
- 앱은 시작할 때 자기 폴더를 PATH 맨 앞에 붙인다. 그래서 앱이 띄우는 에이전트 CLI·`o dispatch`·카드의 `o msg`/`o context`는 `cargo install` 없이도 함께 설치된 `o.exe`를 쓴다. 터미널에서 `o`를 쓰려면 저장소에서 `cargo install --path crates/o-cli`를 하거나 앱 폴더를 사용자 PATH에 더한다.
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

`/PURGE`는 설치 위치 기억 키와 `project_O mirror sync (test|test_1|test_2|main)`이라는 정확한 이름의 예약 작업도 제거한다. 거울 git 저장소는 남긴다. 평소 CLI 폴더(`~/.claude`, `~/.codex`, `~/.gemini`, `~/.agents`), 프로젝트와 저장소, 공유 WebView2 런타임에는 손대지 않는다. 별도 첨부 루트는 현재 없으며 O 상태 폴더 안의 첨부만 함께 지운다. 데이터 루트가 비었거나 링크·junction이면 중단한다. 하위 링크는 대상까지 따라가지 않는다. 제거 중 오류가 나면 앱은 남아 있어 다시 시도할 수 있지만, 앞서 지운 데이터는 복구되지 않는다.

## 업데이트 알림

릴리스 빌드는 커밋 SHA와 UTC 빌드 시각을 포함하며 설정 → 정보에 표시한다. 앱 시작 10초 뒤와 이후 6시간마다 사용자 PC의 `gh`로 고정 저장소 `chan12392/project_O`의 최근 릴리스 20개를 확인한다. `test_1-<yyyyMMdd>-<소문자 sha7>` 프리릴리스 중 릴리스 워크플로(`github-actions[bot]`)가 태그의 커밋으로 올렸고, GitHub가 SHA-256 digest를 준 유효한 `project_O_<숫자.숫자.숫자>_x64-setup.exe`가 있는 가장 최근 게시본을 고른다. 그 커밋이 `test_1`에 포함됐는지(`gh api …/compare/test_1...<sha>`가 identical·behind) 확인하지 못하면 제안하지 않는다. **커밋이 다르고 게시 시각이 현재 빌드보다 나중일 때** 알림을 띄운다. 로컬 빌드는 자동 확인하지 않고 정보 화면에서만 확인한다.

`gh`가 없거나 로그인·권한·네트워크 문제면 자동 확인은 조용히 끝난다. 수동 **업데이트 확인**은 `gh auth login --hostname github.com` 안내와 [릴리스 다운로드 페이지](https://github.com/chan12392/project_O/releases)를 보여 준다. 개발 서버와 데모는 데스크톱 앱에서만 확인 가능하다고 안내한다.

**지금 업데이트**를 눌러야 다운로드한다. `gh release download`가 새 임시 폴더에 받은 설치 파일의 이름·크기와 릴리스 자산의 SHA-256 digest를 검증한 후 대화형 설치기를 열고 앱을 종료한다. 설치 완료 화면에서 앱을 다시 열 수 있다. 기존 설치 위치와 데이터는 유지된다. 다운로드 중 나중에/Esc를 누르면 다운로드 종료 시 설치를 취소한다. 오류는 팝업에 표시하고 재시도할 수 있다. **이 버전 건너뛰기**는 이 PC의 앱 설정에 태그를 기억하며, 수동 확인은 건너뛴 버전도 보여 준다.

다운로드는 셸 없이 고정 인자로 `gh`를 호출한다. O가 인증 파일·토큰을 읽거나 전달하거나 사용자 이메일을 전송하지 않는다. 아직 코드 서명·별도 업데이트 서명은 없으며 크기·digest 검증은 받은 파일이 GitHub에 올라간 자산과 같은지만 확인할 뿐 서명 검증을 대신하지 않는다. 실행한 설치 파일은 `%TEMP%\project_O-update-*`에 남고 Windows 임시 파일 정리 대상이 된다.

## 제거 검증 (개발자)

`pwsh scripts/test-installer.ps1 -Setup <빌드한 setup.exe>`는 앱을 열지 않고 임시 경로에 설치·제거하며 실제 데이터는 건드리지 않는다. 기존 설치가 등록돼 있으면 중단한다. `/PURGE /DATADRYRUN`은 실제 데이터 대신 `%TEMP%\project_O-purge-plan-*\plan.json`에 대상만 기록하고 앱을 제거한다.

통합 시험의 `/PURGE /TESTDATA=<32자리 hex ID>`는 `%LOCALAPPDATA%\project_O-uninstall-tests\<ID>\Local` 및 `Roaming` 아래의 동일한 고정 이름만 삭제한다. 이 모드에서는 실제 예약 작업·설치 위치 기억 키를 삭제하지 않는다. 임의 경로나 상대 경로를 받지 않으며 일반 사용자 명령에는 넣지 않는다. 데이터 도우미는 NSIS와 함께 배포되는 `o-uninstall-data.exe`이고 단독 실행은 데이터 삭제이므로 직접 실행하지 않는다.

## 설치 파일 구성 (고치는 사람용)

| 항목 | 값 |
|---|---|
| Tauri 설정 | `app/src-tauri/tauri.conf.json`의 `bundle`(NSIS만, `installMode: currentUser`, 한국어, 게시자·저작권 `Project O`) + 설치 빌드에서만 합치는 `scripts/installer/tauri.installer.json`(`externalBin: binaries/o`, 사이드카 준비) |
| 사이드카 | `scripts/prepare-sidecar.mjs`(`npm --prefix app run sidecar`)가 release `o.exe`를 `app/src-tauri/binaries/o-<triple>.exe`로 복사(gitignore). 일반 `cargo build`·`tauri dev`에는 필요 없다 |
| NSIS 훅 | `scripts/installer/hooks.nsh` — 기본 폴더를 상태 폴더 밖으로 옮긴다 |
| 설치 폴더 | `%LOCALAPPDATA%\Programs\project_O` (`/D=`로 바꾼 경우 그 폴더) |
| 제거 레지스트리 | `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\project_O` — `UninstallString` = `"<설치 폴더>\uninstall.exe"`, `InstallLocation`, `DisplayVersion` |
| 설치 위치 기억 | `HKCU\Software\Project O\project_O`의 기본값. 다음 설치가 이 폴더를 쓴다. 일반 제거는 남기고 "앱 데이터 삭제"를 고를 때만 지운다 |
| 상태 폴더 | `%LOCALAPPDATA%\project_O`(`store::default_home()`), `%LOCALAPPDATA%\project_O-agents`(깨끗한 에이전트), `%LOCALAPPDATA%\local.project-o.desktop`(WebView2, 식별자 이름) |
| 릴리스 | `.github/workflows/release.yml`: `test_1` push·`test_1`에서의 수동 실행(다른 ref에서 실행하면 job이 건너뛰어 아무것도 게시하지 않음) → 태그 `test_1-<yyyyMMdd(UTC)>-<sha7>` 프리릴리스, 첨부 `project_O_<앱 버전>_x64-setup.exe`, 아티팩트 `project_O-setup`. 같은 날 같은 커밋을 다시 돌리면 파일만 바꾼다 |
| 버전 | 앱 버전은 `app/src-tauri/Cargo.toml`의 `version`(현재 0.3.0)과 `app/package.json`. 크레이트는 워크스페이스 버전을 따른다 |

## 문제 알리기

저장소 Issues에 올리거나 팀 대화에 남긴다. 다음을 함께 적는다.

- 설치 파일 이름(버전)과 받은 곳(프리릴리스 태그 또는 직접 빌드한 커밋)
- Windows 버전, 무엇을 하다가 어떤 화면·메시지가 나왔는지
- `o about --json` 결과(앱 폴더의 `o.exe`로 실행: `& "$env:LOCALAPPDATA\Programs\project_O\o.exe" about --json`)

로그나 상태 폴더 파일을 올릴 때는 계정 이름·이메일·토큰이 없는지 먼저 확인한다.
