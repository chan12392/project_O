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
| `%LOCALAPPDATA%\project_O` | O 상태 폴더(실행 기록·`o.db`·설정, `o-core`의 `store::default_home()`). 정책 보호 대상 | **남김** |
| `%LOCALAPPDATA%\project_O-agents` | 깨끗한 에이전트 프로필(`o agents path`) | **남김** |
| `%LOCALAPPDATA%\local.project-o.desktop` | 창(WebView2) 데이터 | 제거 창에서 "앱 데이터 삭제"를 고를 때만 삭제 |

- Tauri의 사용자별 기본 설치 폴더는 `%LOCALAPPDATA%\<제품 이름>` = `%LOCALAPPDATA%\project_O`로, O 상태 폴더와 겹친다. 그래서 설치 파일이 기본 폴더를 `%LOCALAPPDATA%\Programs\project_O`로 옮긴다(`scripts/installer/hooks.nsh`). 설치 화면에서 상태 폴더를 직접 골라도 같은 곳으로 바꾼다.
- 시작 메뉴에 `project_O`, 바탕 화면에 바로가기가 생긴다(설치 마지막 화면에서 바탕 화면 바로가기를 끌 수 있다).
- 앱은 시작할 때 자기 폴더를 PATH 맨 앞에 붙인다. 그래서 앱이 띄우는 에이전트 CLI·`o dispatch`·카드의 `o msg`/`o context`는 `cargo install` 없이도 함께 설치된 `o.exe`를 쓴다. 터미널에서 `o`를 쓰려면 저장소에서 `cargo install --path crates/o-cli`를 하거나 앱 폴더를 사용자 PATH에 더한다.
- 조용한 설치: `project_O_<버전>_x64-setup.exe /S`. 폴더를 바꾸려면 `/D=<폴더>`를 맨 끝에 따옴표 없이 붙인다.

## 처음 실행

1. 시작 메뉴나 바탕 화면의 `project_O`를 연다.
2. 첫 실행 안내(이후에는 설정 화면)에서 Claude Code·Codex·Antigravity의 설치·로그인 상태를 본다. 설치는 공식 npm 패키지가 있고 npm이 PATH에 있을 때만 앱에서 실행하고, 그 밖에는 공식 설치 안내를 연다.
3. 로그인 버튼은 새 콘솔 창에서 각 CLI의 공식 로그인 명령을 띄운다. 로그인 정보는 각 CLI가 관리하고 O는 읽지 않는다.

## 제거

설정 → 앱 → 설치된 앱 → `project_O` → 제거 (또는 앱 폴더의 `uninstall.exe`, 조용히: `uninstall.exe /S`). 앱 폴더와 바로가기만 지운다. O 상태 폴더와 에이전트 프로필 폴더는 남는다. 완전히 지우려면 앱을 제거한 뒤 두 폴더를 직접 지운다.

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
| 릴리스 | `.github/workflows/release.yml`: `test_1` push·수동 실행 → 태그 `test_1-<yyyyMMdd(UTC)>-<sha7>` 프리릴리스, 첨부 `project_O_<앱 버전>_x64-setup.exe`, 아티팩트 `project_O-setup`. 같은 날 같은 커밋을 다시 돌리면 파일만 바꾼다 |
| 버전 | 앱 버전은 `app/src-tauri/Cargo.toml`의 `version`(현재 0.3.0)과 `app/package.json`. 크레이트는 워크스페이스 버전을 따른다 |

## 문제 알리기

저장소 Issues에 올리거나 팀 대화에 남긴다. 다음을 함께 적는다.

- 설치 파일 이름(버전)과 받은 곳(프리릴리스 태그 또는 직접 빌드한 커밋)
- Windows 버전, 무엇을 하다가 어떤 화면·메시지가 나왔는지
- `o about --json` 결과(앱 폴더의 `o.exe`로 실행: `& "$env:LOCALAPPDATA\Programs\project_O\o.exe" about --json`)

로그나 상태 폴더 파일을 올릴 때는 계정 이름·이메일·토큰이 없는지 먼저 확인한다.
