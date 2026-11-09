# 기여 안내

project_O는 작은 팀이 쓰는 개인 프로젝트다. 버그 제보와 PR을 받지만, 병합은 팀의 브랜치 흐름([AGENTS.md](AGENTS.md))을 따른다.

## 외부 PR (포크)

- 포크 저장소에서 연 PR은 그대로 병합하지 않는다. `.github/workflows/branch-flow.yml`이 포크 브랜치의 PR을 실패시킨다("포크 저장소의 브랜치는 허용하지 않습니다").
- 받기로 한 변경은 팀원이 자기 작업 브랜치(`<개인>-<주제>`)로 가져와 `test_2`로 다시 올린다. 커밋을 그대로 가져오면 원래 작성자 정보가 남는다. 원래 PR에는 다시 올린 PR의 링크를 남기고 닫는다.
- 큰 변경은 PR 전에 이슈로 먼저 알려 주면 좋다.
- 보낸 기여는 프로젝트 라이선스([Apache-2.0](LICENSE))에 따라 받는다.

## 개발 환경

Windows, Rust(stable), Node.js 22.18 이상. macOS(Apple Silicon)는 실험 단계다([README](README.md#쓰기-전에)).

### `o` 설치 (먼저)

git 훅과 에이전트 훅이 PATH의 `o`를 부른다. 저장소를 받은 뒤 가장 먼저 설치한다.

```sh
cargo install --path crates/o-cli   # ~/.cargo/bin/o — 어디서나 `o`
o --help
```

설치한 데스크톱 앱의 폴더(`%LOCALAPPDATA%\Programs\project_O`)를 사용자 PATH에 더해 함께 설치된 `o.exe`를 써도 된다([docs/INSTALL.md](docs/INSTALL.md)). `o`를 고친 뒤에는 `cargo install --path crates/o-cli`로 다시 설치한다.

### 에이전트 훅

이 저장소에서 Claude Code나 Codex를 쓰면 프로젝트 훅이 도구 호출마다 `o hook`을 부른다.

| 파일 | 명령 | `o`가 없거나 훅이 오류로 끝나면 |
|---|---|---|
| `.claude/settings.json` | `o hook`, `"onFailure": "block"` | Bash·PowerShell·Write·Edit·Read·Grep·Glob 등 도구 호출이 모두 막힌다(Claude Code 2.1.295 이상) |
| `.codex/hooks.json` | `o hook \|\| exit 2` | Bash·apply_patch·Write·Edit 호출이 막힌다 |

세션이 시작부터 막히면 `o`가 PATH에 있는지 먼저 확인한다(`o --help`). Codex는 프로젝트 훅을 처음 쓸 때와 훅 명령이 바뀔 때 신뢰 승인을 물을 수 있다.

훅이 막는 것(인증 파일·토큰 접근, 공급자 서버 직접 호출, 권한 확인을 끄는 옵션, 보호 브랜치 커밋·push 등)은 [AGENTS.md](AGENTS.md) "프로바이더 정책 자동 차단"에 있다. 막히면 우회하지 않는다. 규칙이 틀렸다고 생각하면 이슈나 PR로 알려 준다.

### git 훅

```sh
npm install   # 저장소 루트. git config core.hooksPath .githooks
```

`pre-commit`은 `o scan --staged`, `pre-push`는 `o scan --pre-push`를 부른다. 같은 정책 검사를 하고, 보호 브랜치(`test_1`·`test_1_hotfix`·`test_2`·`main_hotfix`·`main`)의 커밋·push를 막는다.

## 검사

PR 전에 로컬에서 돌린다. 개발 설정으로 돌리고 `--release`는 쓰지 않는다(다시 빌드가 크게 느려진다).

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo run -p o-cli -- scan --all   # 전체 파일 정책 검사
npm test                           # 화면 로직 테스트 (루트)
cd app && npm install && npm run check   # 화면 TypeScript
```

화면을 고쳤다면 `cd app && npm run dev`(브라우저, 기록 재생 데모는 `?source=demo`)나 `npm run preview:app`(데스크톱 미리보기 창)으로 확인한다.

CI(`.github/workflows/guard.yml`)는 PR에서만 돈다. PR 없이 push한 브랜치는 CI가 돌지 않는다.

## 커밋·PR

- 커밋 메시지: `<type>: <요약>` (`feat`, `fix`, `docs`, `refactor`, `test`, `chore`).
- PR 본문은 `.github/pull_request_template.md`를 채운다. 무엇을 바꿨는지와 어떻게 확인했는지를 적는다.
- 구독 계정 인증 정보·토큰·`.env`는 커밋하지 않는다. 테스트·데모 데이터에 실제 계정명·이메일을 넣지 않는다.
- 공용 경계(기술 스택, 공용 데이터 모델, 프로바이더 연결 방식, 라우팅 단위, 화면 최상위 구조)를 바꾸면 `docs/decisions/`에 ADR을 더한다. 모듈 지도와 새 프로바이더·화면·명령 체크리스트는 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
- 정책 규칙(`crates/o-guard/src/rules.rs`)과 상한(`crates/o-core/src/policy.rs`의 `LIMITS`)을 완화하는 변경은 ADR과 함께 올린다.
