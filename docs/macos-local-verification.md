# macOS 로컬 검증 절차

이 절차는 macOS 담당자가 전달받은 **현재 제품 소스**를 독립 임시 폴더에서 검사한다. 가짜 자식만 실행하며 계정·구독·네트워크·GUI·관리자 권한·Git worktree를 사용하지 않는다. 실제 Mac에서는 아직 실행하지 않았고, 현재 WIP는 아래 제품 검사의 일부가 실패할 것으로 예상된다. 임시 자료는 삭제하지 않으며 경로를 출력한다.

## 준비

- Python 3, Rust 및 대상 프로젝트의 의존성이 이미 로컬 Cargo 캐시에 있어야 한다. 없는 경우 설치하거나 네트워크에 연결하지 말고 중단한다.
- `scripts/macos-process-probe.py`, `scripts/macos-resource-probe.rs`와 검사 대상 Project O 소스가 필요하다. 소스는 읽기만 하며 별도의 Cargo 패키지와 빌드 결과만 임시 폴더에 만든다.
- 실제 사용자 상태 폴더·홈 폴더·프로젝트를 테스트 대상으로 지정하지 않는다. 아래 `TMPDIR`와 `CARGO_TARGET_DIR`은 이 셸과 자식에서만 유효하고 실제 설정은 변경하지 않는다.

```sh
# 각 경로를 자신이 받은 소스의 절대 경로로 바꾼다.
PROBE_SOURCE='/절대경로/project_O'
PRODUCT_SOURCE='/절대경로/project_O'
test -f "$PROBE_SOURCE/scripts/macos-process-probe.py" || exit 1
test -f "$PRODUCT_SOURCE/crates/o-core/Cargo.toml" || exit 1
test "$(uname -s)" = Darwin || exit 1
O_PROBE_DIR=$(mktemp -d "${TMPDIR:-/tmp}/o-macos-verify.XXXXXXXX") || exit 1
mkdir -p "$O_PROBE_DIR/tests" "$O_PROBE_DIR/tmp" "$O_PROBE_DIR/target" || exit 1
export TMPDIR="$O_PROBE_DIR/tmp" CARGO_TARGET_DIR="$O_PROBE_DIR/target" CARGO_NET_OFFLINE=true
printf '검사 자료: %s\n' "$O_PROBE_DIR"
python3 -B "$PROBE_SOURCE/scripts/macos-process-probe.py"
```

OS 실험은 직접 만든 가짜 부모에 파이프로 중지를 요청한다. 부모가 시작한 `setsid` 자손은 3초 뒤 스스로 끝나며 PID·프로세스 그룹에 신호를 보내지 않는다. 부모가 끝나도 자손이 남은 동안 출력 파이프가 열려 있는 경우와 닫힌 경우를 각각 관찰하므로, 어느 경우도 자손 종료의 증거로 간주하지 않는다. 정상 종료와 해당 임시 볼륨의 대소문자·Unicode 동등성도 출력한다. 오류나 시간 초과는 불명이며 외부 프로세스를 종료하지 않는다.

`launchd`의 기본 종료를 다른 세션으로 분리된 자손의 종료 증거로 쓰지 않는다. 별도 정적 검토에 따르면 [Apple 공개 XNU 코드](https://github.com/apple/darwin-xnu/blob/main/bsd/kern/kern_exec.c)의 다른 coalition 배정 경로에는 권한 또는 entitlement 확인이 있다. coalition은 실제 권한·이탈 방지·종료 확인을 검증해야 할 후보이며, 일반 사용자 환경에서 사용 가능하다고 가정하지 않는다. 이 절차에서는 해당 외부 코드를 열거나 coalition을 조작하지 않는다.

## 실제 예약 및 카드 코드 검사

아래는 검사 대상의 `o-core`를 경로 의존성으로 읽는 독립 Cargo 패키지다. `HOME`이나 기존 인증 정보를 변경하지 않으며, 가짜 카드·소유자 자식에게는 비운 환경만 전달한다. `--offline`이 실패하면 의존성 부족으로 기록하고 멈춘다.

```sh
cp "$PROBE_SOURCE/scripts/macos-resource-probe.rs" "$O_PROBE_DIR/tests/macos_resource_probe.rs" || exit 1
O_PRODUCT_CORE="$PRODUCT_SOURCE/crates/o-core" O_PROBE_MANIFEST="$O_PROBE_DIR/Cargo.toml" python3 -B - <<'PY'
import json, os, pathlib
core = pathlib.Path(os.environ['O_PRODUCT_CORE']).resolve(strict=True)
manifest = pathlib.Path(os.environ['O_PROBE_MANIFEST'])
manifest.write_text('[package]\nname = "o-macos-probe"\nversion = "0.0.0"\nedition = "2021"\n\n[dependencies]\nchrono = { version = "0.4", default-features = false, features = ["clock", "std"] }\no-core = { path = ' + json.dumps(str(core), ensure_ascii=False) + ' }\n', encoding='utf-8')
PY
test -f "$O_PROBE_DIR/Cargo.toml" || exit 1
cargo test --offline --manifest-path "$O_PROBE_DIR/Cargo.toml" --test macos_resource_probe -- --list
```

목록에 다음 열한 검사가 정확히 있는지 먼저 확인한다. `fake_owner`, `fake_card`, `fake_lead`는 검사에서 직접 실행하는 보조 항목이다. 목록이 다르거나 필터를 무시하면 중단하고 전체 테스트를 실행하지 않는다. 각 명령은 같은 임시 패키지에서 하나의 검사만 실행하며, 실패해도 다음 항목을 따로 기록한다.

```sh
cargo test --offline --manifest-path "$O_PROBE_DIR/Cargo.toml" --test macos_resource_probe macos_case_alias_reservation -- --exact --nocapture
cargo test --offline --manifest-path "$O_PROBE_DIR/Cargo.toml" --test macos_resource_probe macos_unicode_alias_reservation -- --exact --nocapture
cargo test --offline --manifest-path "$O_PROBE_DIR/Cargo.toml" --test macos_resource_probe macos_ambiguous_spawn_recovery -- --exact --nocapture
cargo test --offline --manifest-path "$O_PROBE_DIR/Cargo.toml" --test macos_resource_probe macos_unstarted_owner_recovery -- --exact --nocapture
cargo test --offline --manifest-path "$O_PROBE_DIR/Cargo.toml" --test macos_resource_probe macos_card_normal -- --exact --nocapture
cargo test --offline --manifest-path "$O_PROBE_DIR/Cargo.toml" --test macos_resource_probe macos_card_stop -- --exact --nocapture
cargo test --offline --manifest-path "$O_PROBE_DIR/Cargo.toml" --test macos_resource_probe macos_card_escaped_closed -- --exact --nocapture
cargo test --offline --manifest-path "$O_PROBE_DIR/Cargo.toml" --test macos_resource_probe macos_card_escaped_pipe -- --exact --nocapture
cargo test --offline --manifest-path "$O_PROBE_DIR/Cargo.toml" --test macos_resource_probe macos_lead_normal -- --exact --nocapture
cargo test --offline --manifest-path "$O_PROBE_DIR/Cargo.toml" --test macos_resource_probe macos_lead_stop -- --exact --nocapture
cargo test --offline --manifest-path "$O_PROBE_DIR/Cargo.toml" --test macos_resource_probe macos_lead_escaped_descendant -- --exact --nocapture
```

- 경로 검사는 먼저 실제 볼륨에서 두 이름의 동등성을 확인하고, 둘 다 아직 없는 하위 경로를 같은 쓰기 자원으로 예약할 수 있는지 제품 API로 검사한다. 동등한 이름이 둘 다 허용되면 실패다. 다른 볼륨의 동작을 대변하지 않는다.
- 두 소유자 검사는 직접 만든 자식이 살아 있는 동안 복구가 거부되는지 확인한다. 시작 의도는 기록됐지만 종료 증거가 없는 경우 소유자가 4초 뒤 스스로 끝나도 불명을 보존하고 복구·충돌 예약을 거부해야 한다.
- 별도의 시작 전 예약은 작업이 시작되지 않았다는 기록과 소유자 신원·종료가 모두 확인되면 복구와 재예약이 가능해야 한다. 신원이나 종료를 증명할 수 없으면 예약은 불명으로 유지하고 검사는 실패로 기록한다.
- 카드 검사는 제품의 `run_plan`과 `stop_run`에 가짜 실행 파일을 주입한다. 정상 종료 후 성공과 예약 해제, 중지 요청 후 신속 종료와 예약 해제를 각각 요구한다. 가짜 카드는 중지가 작동하지 않아도 5초 뒤 스스로 끝나며 외부 PID에는 신호를 보내지 않는다. 현재 macOS 경로는 이 요구를 충족하지 못할 수 있으므로 실패를 성공으로 바꾸지 않는다.
- 두 자손 카드 검사는 `setsid` 자손이 stdout을 닫거나 보유한 채 살아 있는 동안 성공으로 표시하거나 공용 예약을 해제하지 않는지 확인한다. 자손은 5초 뒤 스스로 종료한다.
- 총괄 검사는 가짜 직접 자식의 정상 종료·중지 후 다음 턴이 가능한지, 표준 출력을 닫고 `setsid`로 분리된 자손이 살아 있는 동안 불명 표식을 유지하고 새 턴을 차단하는지 확인한다. 분리된 가짜 자손은 5초 뒤 스스로 끝나며 어떤 PID에도 신호를 보내지 않는다.
- 실행기 강제 종료 후 복구 및 재시작과 복구·예약의 경쟁은 이 두 파일로 자동 검증하지 않는다. 별도 가짜 프로세스 통합 검사에서 같은 조건을 검증해야 하며, 소유 핸들 없이 기록된 PID를 종료하거나 구형 예약을 수동 삭제하지 않는다.

## 로그인 연결: 정적 확인과 별도 실기 검증

정적 코드에서 앱의 `open_login`은 `agents::open_login`과 `sys::proc::spawn_console`을 거쳐 macOS Terminal을 요청한다. 환경은 일회성 로컬 소켓으로 전달하고 명령줄에 쓰지 않는다. Windows에서 인용과 환경 분리를 검사했지만 실제 macOS의 Terminal 자동화 권한과 창 동작은 아직 검증하지 않았다. `o agents login <provider>`는 호출한 터미널에 연결된 경로다. 실제 공식 CLI 로그인·구독·인증 변경은 이 절차에서 실행하지 않는다.

이미 빌드된 검사 권한이 있는 macOS용 `o` 실행 파일이 있다면 아래처럼 임시 디렉터리에 복사하고 가짜 `codex`만 PATH에 두어 터미널 경로를 확인한다. 대화형 터미널에서만 실행하고 `FAKE-TTY-OK`와 종료 코드 0을 확인한다. 이 가짜 CLI는 고정 문자열만 받고 네트워크나 인증 파일을 쓰지 않는다.

```sh
O_BINARY='/검사권한이-있는-별도-경로/o'
test -f "$O_BINARY" || exit 1
mkdir -p "$O_PROBE_DIR/bin" "$O_PROBE_DIR/home" "$O_PROBE_DIR/statebase"
cp "$O_BINARY" "$O_PROBE_DIR/bin/o"
cat > "$O_PROBE_DIR/bin/codex" <<'SH'
#!/bin/sh
[ "$#" -eq 1 ] && [ "$1" = login ] || exit 10
[ -t 0 ] && [ -t 1 ] || exit 11
printf '가짜 확인 문자열 PROBE 입력: '
IFS= read -r answer
[ "$answer" = PROBE ] || exit 12
printf 'FAKE-TTY-OK\n'
SH
chmod u+x "$O_PROBE_DIR/bin/codex"
env -i HOME="$O_PROBE_DIR/home" LOCALAPPDATA="$O_PROBE_DIR/statebase" PATH="$O_PROBE_DIR/bin:/usr/bin:/bin" "$O_PROBE_DIR/bin/o" agents login codex
```

이 명령은 터미널에서 호출한 `o agents login` 경로만 검증한다. 새 Terminal 창을 여는 검사가 허용된 경우에만 다음 명령을 실행하고, 새 창에서 `PROBE`를 입력해 `FAKE-TTY-OK`가 나오는지 확인한다. macOS 자동화 권한을 거부한 경우 오류가 표시되는지도 별도로 기록한다. 이 검사는 위에서 만든 가짜 CLI만 사용한다.

```sh
env -i HOME="$O_PROBE_DIR/home" LOCALAPPDATA="$O_PROBE_DIR/statebase" PATH="$O_PROBE_DIR/bin:/usr/bin:/bin" "$O_PROBE_DIR/bin/o" agents open-login codex --clean
```

실제 계정으로 공식 CLI 로그인을 확인해야 한다면 사용자가 직접 기존 공식 로그인 절차를 수행하고, GUI에 입력창이 보이는지와 터미널에서 이어지는지만 관찰한다. 인증 파일·토큰·계정 내용은 읽거나 복사하거나 보고하지 않는다.

## .app과 아키텍처 확인

`app/src-tauri/tauri.macos.conf.json`은 `.app`과 `binaries/o`를 선언하고, `scripts/prepare-sidecar.mjs`는 Tauri 또는 Cargo가 지정한 타깃을 우선 사용해 `o`를 빌드하며 지정이 없을 때 호스트 타깃을 사용한다. 결과는 소스 트리의 `app/src-tauri/binaries`에 쓰므로 기존 WIP를 그대로 둬야 하는 이 절차에서는 원본에서 표준 빌드를 실행하지 않는다. 아래는 담당자가 이미 가지고 있는, 검사 권한이 있는 별도 `.app`을 읽기만 하는 검사이며 앱을 열지 않는다.

```sh
APP='/검사권한이-있는-별도-경로/Project O.app'
test -d "$APP/Contents" || exit 1
APP_EXE=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$APP/Contents/Info.plist") || exit 1
case "$APP_EXE" in ''|*/*|..|.) echo '실행 파일 이름 불명'; exit 1;; esac
test -f "$APP/Contents/MacOS/$APP_EXE" || exit 1
test -f "$APP/Contents/MacOS/o" || { echo 'o 동반 파일 없음 또는 배치 미확인'; exit 1; }
uname -m
rustc -vV
file "$APP/Contents/MacOS/$APP_EXE" "$APP/Contents/MacOS/o"
lipo -archs "$APP/Contents/MacOS/$APP_EXE"
lipo -archs "$APP/Contents/MacOS/o"
```

Apple Silicon에서는 두 파일에 `arm64`, Intel에서는 `x86_64`가 있어야 한다. Rosetta나 교차 빌드를 네이티브 빌드로 오인하지 않도록 `rustc -vV`의 host와 실행 환경을 함께 기록한다. 표준 빌드까지 확인할 때는 Git worktree가 아닌, 인증 파일과 `.git`을 포함하지 않는 검사 전용 소스 사본을 `$O_PROBE_DIR/source`에 별도 준비하고 설치된 `node_modules`와 Cargo 캐시가 모두 있는지 확인한다. 네트워크가 차단된 환경에서 아래 명령을 실행하며, 사본이나 의존성이 없으면 설치하지 말고 미검증으로 남긴다.

```sh
test -f "$O_PROBE_DIR/source/app/src-tauri/tauri.macos.conf.json" || exit 1
test -x "$O_PROBE_DIR/source/app/node_modules/.bin/tauri" || exit 1
(cd "$O_PROBE_DIR/source/app" && npm run tauri -- build --config src-tauri/tauri.macos.conf.json)
```

이 명령은 사본의 `binaries`와 임시 빌드 폴더에 쓴다. 빌드된 `.app`을 위 명령으로 확인하고 각 Mac에서 빌드 종료 코드와 결과를 기록한다. 이 파일 검사만으로 표준 빌드 성공이나 GUI에서 sidecar 실행·로그인이 작동함을 증명하지 못한다. Apple Silicon과 Intel 각각의 실제 빌드는 아직 검증하지 않았다.

## 결과 기록

macOS 버전·CPU 아키텍처·검사한 소스의 커밋 및 WIP 여부, 대상 볼륨의 관찰 결과, 각 명령의 통과·실패·불명, 재현 조건과 임시 폴더 경로를 기록한다. 이 실험은 다른 사용자·프로세스의 우회, 원격 서비스에 이미 전달된 비동기 작업, 전원 중단 후 내구성을 보장하지 않는다.
