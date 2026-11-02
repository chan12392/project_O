# macOS에서 Project O 실행하기

Windows용 `.exe`는 맥에서 실행하지 않습니다. 맥에서 소스를 빌드하면 `project_O.app`이 만들어집니다. 앱에는 작업 실행용 `o` 프로그램도 포함됩니다.

## 준비

- macOS 11 이상, Node.js 22.18 이상, Rust stable, Apple Command Line Tools.
- Apple 개발 도구: `xcode-select --install`. 설치 여부는 `xcode-select -p`로 확인합니다.
- Rust는 [공식 설치 안내](https://www.rust-lang.org/tools/install)를 따릅니다. Homebrew의 `rustup`을 썼다면 `brew --prefix rustup`의 `bin`이 PATH에 있어야 합니다.
- 이 비공개 저장소를 받을 권한이 필요합니다. 사용자 로그인 정보나 다른 사람의 프로필을 복사하지 않습니다.

## 앱 만들기

저장소 루트에서 실행합니다.

```sh
npm ci
npm ci --prefix app
cd app
npm run tauri build -- --bundles app
```

결과는 저장소의 `target/release/bundle/macos/project_O.app`입니다. Finder에서 이 앱을 열거나 응용 프로그램 폴더에 복사합니다. 빌드한 맥과 같은 CPU 종류용 앱이 만들어집니다. Apple Silicon에서 만든 앱을 Intel 지원 검증으로 간주하지 않습니다.

배포용 디스크 이미지가 필요하면 같은 폴더에서 실행합니다.

```sh
npm run tauri build -- --bundles dmg
```

결과는 `target/release/bundle/dmg/` 아래에 있습니다. Apple 개발자 서명·공증은 별도 배포 작업입니다. 로컬 빌드 성공은 다른 사람의 맥에서 경고 없이 설치된다는 뜻이 아닙니다. 보안 경고를 자동으로 우회하거나 격리 속성을 제거하지 않습니다.

## 처음 실행

1. 연결 화면에서 설치된 에이전트가 표시되는지 확인합니다. 앱은 Finder의 짧은 PATH에 Homebrew·`~/.local/bin`·`~/.cargo/bin` 위치를 더해 찾습니다. 사용자 셸 설정 파일이나 인증 파일을 읽지 않습니다. 다른 위치에 설치한 CLI는 PATH가 설정된 터미널에서 앱을 실행하거나 표준 설치 위치를 사용하세요.
2. 로그인 버튼은 Terminal 창을 엽니다. 로그인은 공식 CLI와 브라우저에서 직접 완료합니다. 앱은 비밀번호·토큰을 받지 않습니다.
3. 기본값인 깨끗한 프로필은 평소 로그인과 분리돼 있어 별도 로그인이 필요할 수 있습니다. `연결되지 않음`은 평소 계정이 로그아웃됐다는 뜻이 아닙니다.
4. 시험용 빈 프로젝트를 등록하고 작업을 요청합니다. 계획을 확인한 다음 실행합니다.

## 업데이트와 제거

- 이 맥 빌드는 자동 업데이트를 제공하지 않습니다. 새 맥용 앱을 받아 종료된 앱을 교체합니다. Windows 설치파일은 제안하거나 실행하지 않습니다.
- 앱을 종료하고 Finder에서 `.app`을 휴지통으로 옮기면 앱만 제거됩니다.
- 실행 기록은 `~/.local/state/project_O`, 분리된 에이전트 프로필은 `~/.local/state/project_O-agents`에 있습니다. 앱 제거만으로 이 데이터는 지워지지 않습니다. 정확한 경로는 설정 → 정보에서 확인합니다.
- Windows용 PowerShell 제거 명령은 맥에서 사용하지 않습니다.

## 검사

```sh
# 저장소 루트
npm test
npm run check
npm --prefix app run check
# macOS의 /var 별칭 대신 실제 임시 경로를 써서 첨부파일 보안 검사와 충돌하지 않게 합니다.
export TMPDIR="$(cd "$TMPDIR" && pwd -P)"
cargo test --workspace --release
cargo clippy --workspace --release -- -D warnings
```

실제 앱의 시작·설정·프로젝트 전환과 실제 AI 로그인/실행을 각각 기록합니다. 브라우저의 `?source=demo`는 예시 재생이므로 실제 실행 검증을 대신하지 않습니다.

현재 macOS 프로세스 종료 처리는 Windows의 job object와 동등한 보장을 제공하지 않습니다. 비정상 종료 뒤 모든 하위 프로세스가 함께 종료되는지는 별도 검증이 필요합니다.
