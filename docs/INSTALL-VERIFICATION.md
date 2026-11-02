# 설치 제거·업데이트 검증

2026-11-02 Windows x64, 로컬 빌드. 앱 GUI는 실행하지 않았다. 기능 커밋 `75b28a0`에 `origin/test_2`의 모듈화 `d31806d`를 병합한 `8e3eecd`에서 필수 검사를 다시 통과했다.

## 사용 경로

`o`가 PATH에 없어도 PowerShell에서 실행할 수 있다. 데이터 보존:

```powershell
Start-Process -FilePath ((Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\project_O').UninstallString.Trim('"')) -ArgumentList '/S' -WindowStyle Hidden -Wait
```

앱 데이터까지 제거:

```powershell
Start-Process -FilePath ((Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\project_O').UninstallString.Trim('"')) -ArgumentList '/S /PURGE' -WindowStyle Hidden -Wait
```

두 명령은 설정 → 정보에도 표시하고 복사할 수 있다. 제거기 체크박스도 `/PURGE`와 같은 검증된 경로만 삭제한다.

업데이트는 고정 저장소 `chan12392/project_O`에서 `test_1-<yyyyMMdd>-<sha7>` 형식과 정상 설치 자산을 갖춘 최신 pre-release를 고른다. 빌드 SHA가 다르고 게시 시각이 빌드보다 늦으면 시작 10초 후·6시간마다 알리며, SHA가 없는 로컬 빌드는 수동 확인만 한다. 지금 업데이트를 누른 뒤에만 `gh release download`로 고유 임시 폴더에 받고 파일명·크기·링크 여부를 확인하여 대화형 설치기를 연 다음 앱을 종료한다. 나중에·Esc는 설치를 미루며 건너뛴 태그는 로컬 설정에 저장한다.

## 설치·제거

`pwsh scripts/build-installer.ps1`로 생성한 `target/release/bundle/nsis/project_O_0.3.0_x64-setup.exe`에 `scripts/test-installer.ps1`을 실행했다.

- SHA256: `EBC8134199DDFC82BAC0D920CB988C77C6227CA28E56593CD1CD8591E7624DE2` (46.22 MiB).
- 공백을 포함한 임시 경로에 `/S /D=<경로>` 설치: 앱, `o.exe`, `o-uninstall-data.exe`, 제거기, 시작 메뉴·바탕 화면 바로가기, 제거 레지스트리 확인.
- 문서의 레지스트리 기반 한 줄 명령 `/S`: 앱 폴더·바로가기·제거 키 삭제, 데이터 보존. 별도 프로세스가 잡고 있던 번들 `o.exe` 파일 잠금도 해제.
- `/S /PURGE /TESTDATA=<ID>`: 가짜 Local의 `project_O`, `project_O-agents`, `local.project-o.desktop`와 가짜 Roaming의 `local.project-o.desktop`만 삭제.
- 가짜 일반 CLI 폴더 4개·저장소·프로젝트의 sentinel 파일과 데이터 하위 junction 대상 보존. 데이터 루트 자체가 junction이면 도우미가 거부.
- `/S /PURGE /DATADRYRUN`: 실제 데이터 4개 경로와 설치된 거울 작업의 삭제 계획만 생성. 실제 데이터와 예약 작업 보존.
- 시험 전 설치 위치 기억 키 복구. 실제 로그인 파일은 읽지 않음.

기계 판독 증거: `%LOCALAPPDATA%\project_O-uninstall-tests\3446b6a281594d749d5a20e175bb071b\evidence.json`. 각 설치·제거 exit code는 0이며 실패 시 앱 폴더·키가 남는 경우까지 assert로 감지한다. 시험 데이터와 증거는 재검토를 위해 남겨 두었다.

## 코드·화면

| 검사 | 결과 |
|---|---|
| `cargo test --workspace` | 158 통과, 기존 콘솔 창 시험 1개 ignored |
| `cargo clippy --workspace --all-targets -- -D warnings` | 통과 |
| `npm test` | 25 통과 |
| 루트·앱 `tsc --noEmit` | 모두 통과 |
| `npm --prefix app run build` | 통과 |
| `cargo run --release -p o-cli -- scan --all` | 296개 파일 통과 |
| `pwsh scripts/build-installer.ps1` | 통과 |
| `pwsh scripts/test-installer.ps1 -Setup target/release/bundle/nsis/project_O_0.3.0_x64-setup.exe` | 세 가지 제거 시험 모두 통과 |
| `gh api repos/chan12392/project_O/releases?per_page=5` | 정상 응답 `[]`; fixture JSON으로 선택·비교 시험 |

업데이트 fixture 시험은 게시 시각·동일 커밋·draft·정식 릴리스·설치 파일 없는 릴리스·잘못된 날짜·경로 이동·옵션 삽입·와일드카드·파일 크기를 검사한다. 저장소는 고정 상수이며 새 임시 폴더에서만 다운로드한다.

beakho-design → Impeccable context/harden 절차로 기존 Orca 토큰을 유지했다. Ego Lite에서 정보 화면, 데모·개발 서버 안내, 오류, 안전한 일반 텍스트 변경 내용, 긴 내용 스크롤, 처음·마지막 Tab 순환, Esc와 포커스 복귀, 건너뛰기 저장을 확인했다. 1100×700에서 모달은 576×524.5이고 가로 넘침이 없다. 실제 computed style 기준 본문 대비 17.37:1, 설명 대비 8.72:1.

초기 검사에서 드러난 NSIS 인용, 모달 Tab 순환, 모듈 병합 후 함수 경로 문제를 수정했다. 최종 필수 검사의 종료 코드는 모두 0이다. 화면 검사 결과는 동작 변경 없이 경로만 옮긴 컴포넌트에 재사용했으며, 병합 후 단위 검사·타입 검사·빌드·설치 시험은 다시 실행했다.

## 검증 경계

- 공개 릴리스가 없어 실제 GitHub 설치 파일 다운로드·대화형 업데이트 완료는 실행하지 않았다. 앱 GUI도 요청에 따라 실행하지 않았다.
- 실제 데이터·로그인·예약 작업 삭제는 실행하지 않았다. 실제 경로는 dry-run, 삭제는 격리된 가짜 데이터로 확인했다.
- Ego Lite 창이 최소화되어 스크린샷 검토는 불가했다. DOM·키보드·실제 레이아웃·색상 계산으로 확인했다.
- 설치 파일은 서명되지 않았다. 파일명·크기 검증이 코드 서명 검증을 대신하지 않는다.
