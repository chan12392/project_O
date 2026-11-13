# ADR-0043: 2026-11-11 보안 감사 후속 — 에이전트 경계, 카드 우편함, 릴리스 증명

- 상태: 채택 (2026-11-11, 대표 지시로 구현과 함께)
- 날짜: 2026-11-11
- 별도 승인 대기 없음 (ADR-0012)
- 관련: [ADR-0002](0002-security-hardening.md)(카드 권한), [ADR-0016](0016-devin.md)(Devin), [ADR-0038](0038-ci-release-gate-and-signing.md)(릴리스), [ADR-0042](0042-terminal-agents.md)(터미널 에이전트)

## 배경

2026-11-11 보안 감사가 공용 경계의 빈틈을 지적했다. 구현 결함은 ADR 없이 고쳤고(`4a904d0`: C3·H2·H6·H7·M5), 아래는 공용 경계(카드 권한, 프로바이더 연결, 가드 규칙, 릴리스·병합 흐름)를 바꾸는 것이라 여기 기록한다.

| 감사 항목 | 문제 |
|---|---|
| C1 | "에이전트 안인가"를 환경 변수(`O_AGENT_ROLE`·`O_CARD_ID`)로만 판정했다. 카드의 자식 프로세스가 변수를 지우고 `o`를 부르면 사람 전용 명령이 열렸다 |
| C2 | 업데이터가 믿는 것(릴리스 작성자 `github-actions[bot]`, 태그, digest)을 저장소의 아무 워크플로나 만들 수 있었다 |
| C4 | Devin 카드는 project_O 프로필 계획에서도 실행기의 환경 변수를 전부 물려받았다 |
| C5 | `o msg`를 위해 Codex 카드 샌드박스에 실행 폴더 전체를 쓰기로 열었다. 카드가 `events.jsonl`·`plan.json`·`runner.json`을 직접 고칠 수 있었다 |
| H1 | 총괄 터미널의 `plans/` 폴더에 놓인 파일이 총괄의 계획처럼 보였다 |
| H3 | 가드를 켜는 파일(`.claude/settings.json`·`.codex/hooks.json`·`.githooks/`)을 에이전트가 고칠 수 있었다 |
| H4 | 필수 검사 `check`가 PR이 가져온 워크플로 파일로 돌았다 |
| H5 | 코드 서명 비밀이 저장소 비밀이라 모든 브랜치의 워크플로가 읽을 수 있었다 |

## 결정

### 1. 에이전트 경계: 환경 변수 + Windows 작업 개체 (C1)

- 카드와 총괄 프로세스는 이름 있는 작업 개체(job object) 안에서 시작한다. 카드는 `Global\project_O-job-<실행·카드 표식>-<32hex>`, 총괄(채팅 턴과 총괄 터미널)은 `Global\project_O-lead-<32hex>`. 자식 프로세스는 작업 개체를 벗어날 수 없다.
- `o`는 "에이전트 안인가"를 **환경 변수 또는 작업 개체**로 판정한다(`sys::agent_job::current`, `agents::agy::inside_agent`·`inside_card`). 자기 프로세스가 위 이름의 작업 개체에 들어 있으면 변수가 지워져 있어도 에이전트다.
- 카드 작업 개체 안에서 `o msg`의 신원(`Caller::from_env`)은 작업 개체의 표식과 `O_RUN_ID`·`O_CARD_ID`가 같을 때만 받는다. 다른 카드·다른 실행을 사칭한 변수는 거부한다.
- 사람 전용 목록에 `o stop`, `o lead stop|stop-all|forward`, `o term`의 모든 동사(읽기·목록·크기 변경 포함)를 더한다.
- Windows가 아닌 곳은 환경 변수만 본다.

### 2. Codex 카드의 우편함 (C5)

- YOLO가 아닌 Codex 카드에 여는 쓰기 폴더는 실행 폴더가 아니라 그 카드의 우편함 `<실행 폴더>/mailbox/<카드>` 하나다(`--add-dir`, 환경 변수 `O_MSG_DIR`).
- 카드 안의 `o msg`는 우편함에 요청 파일(`q-<일회용 값>.json`, 인자 목록만)을 쓰고 답(`r-<일회용 값>.json`)을 기다린다. 샌드박스 밖의 카드 실행기가 요청을 읽어 **자기가 시작한 카드의 이름으로** 팀 명령을 실행한다. 요청은 누가 보냈는지 말할 수 없고, 정책 검사(`team_command::run`)는 실행기 쪽에서 전과 같이 거친다.
- 실행 폴더의 `events.jsonl`·`plan.json`·`runner.json`·다른 카드의 `outbox`는 Codex 샌드박스의 쓰기 범위 밖이다.

### 3. Devin 카드의 환경 (C4)

- Devin은 설정 폴더를 옮길 수 없어 평소 프로필로 도는 것은 그대로다(ADR-0016, ADR-0042 §5).
- project_O 프로필 계획의 Devin 카드는 환경 변수를 전부 물려받지 않는다. 시스템 변수, 홈 폴더 위치(`USERPROFILE`·`HOME` 등), `XDG_`·`DEVIN_`·`WINDSURF_`로 시작하는 것, Devin의 인증 변수만 넘긴다(`clean::own_home_environment`). 안내문에 이 사실을 더한다.
- Devin 카드를 project_O 프로필 계획에서 위반으로 거부하지는 않는다(지금 쓰는 흐름을 막지 않기 위해).

### 4. 총괄 터미널의 계획 파일 (H1)

- O는 계획 파일을 총괄 터미널이 썼는지 확인할 수 없다(같은 사용자가 쓸 수 있는 폴더). 그래서 출처를 주장하지 않는다: 계획 카드 첫 줄에 "계획 파일 <이름>에서 읽은 계획입니다. 총괄 터미널이 쓴 것인지 O는 확인하지 못합니다. 카드와 작업 내용을 확인한 뒤 실행하세요."를 보인다.
- 터미널 계획은 YOLO에서도 자동 실행하지 않는다. 사람이 "실행"을 누른다.

### 5. 가드 파일 (H3)

- 에이전트 훅은 `.claude/settings.json`·`.claude/settings.local.json`·`.codex/hooks.json`·`.githooks/*`를 고치는 편집 도구 호출을 막고, 이 이름이 들어간 셸 명령은 읽기 전용 명령일 때만 통과시킨다(`rules::GUARD_FILE`·`GUARD_FILE_NAMED`). 저장소와 무관하게 적용한다. 예외는 `git config core.hooksPath .githooks`(훅을 켜는 쪽)뿐이다.
- 이 파일들은 사람이 고친다.
- `WebFetch`의 주소도 코드 규칙(프로바이더 서버 직접 호출)으로 검사하고, `o scan`은 NUL 바이트가 든 파일을 건너뛰지 않고 NUL을 뺀 내용으로 검사한다(M14).

### 6. 릴리스 증명 (C2, H5, M10)

- `release.yml`은 설치 파일에 GitHub 빌드 증명(`actions/attest-build-provenance`)을 만든다. 증명의 서명 인증서에는 워크플로 파일·ref·커밋이 GitHub에 의해 적히며 워크플로가 위조할 수 없다.
- 업데이터는 내려받은 설치 파일을 `gh attestation verify --repo chan12392/project_O --signer-workflow …/release.yml --source-ref refs/heads/test_1 --source-digest <릴리스 커밋> --deny-self-hosted-runners`로 확인한 뒤에만 실행한다. 확인하지 못하면(증명 없음, `gh` 오류, 시간 초과) 설치하지 않는다. `scripts/release-gate.ps1` 검사 7도 같은 확인을 한다.
- 이 버전 이전의 릴리스는 증명이 없어 새 앱이 설치하지 않는다. 다음 `test_1` 릴리스부터 다시 업데이트된다.
- 서명 job은 `release` 환경에서 돈다. **대표가 저장소 설정에서** 이 환경을 `test_1` 브랜치로 제한하고 `PROJECT_O_SIGN_*`를 저장소 비밀에서 환경 비밀로 옮긴다. 워크플로 파일만으로는 강제되지 않는다.
- 설치 파일 빌드는 `npm ci --ignore-scripts`와 `cargo --locked`를 쓴다.

### 7. PR 검사 (H4)

- `branch-flow.yml`은 `pull_request_target`으로 돈다. PR의 base 브랜치에 있는 워크플로 파일이 실행되므로 PR이 자기 검사를 고칠 수 없다. PR의 코드를 체크아웃하거나 실행하지 않는다.
- 개인·작업 브랜치에서 올라온 PR이 `.github/` 아래를 바꾸면 대표(`chan12392`)가 연 PR이어야 한다. `guard.yml`(`policy`·`windows`)은 PR의 코드를 빌드해야 해서 PR이 가져온 워크플로로 돌기 때문이다. 보호 브랜치 사이의 PR(`test_2` → `test_1` 등)과 동기화 PR은 이 검사를 받지 않는다.

### 8. 그 밖에

- 창은 앱 자신의 페이지에만 머문다(`tauri://localhost`, `http(s)://tauri.localhost`, 개발 빌드의 개발 서버). 다른 주소로의 이동은 거부한다(M13).
- 거울 동기화는 `origin`이 project_O 저장소일 때만 돈다(M12).
- 웹 자료 받기는 이름을 한 번 풀어 내부 주소가 아님을 확인한 그 주소로만 연결한다(`curl --resolve`).

## 한계 (막지 못하는 것)

- **같은 Windows 사용자 권한의 우회**: 카드가 다른 서비스(예약 작업, WMI)를 통해 작업 개체 밖에서 프로세스를 띄우거나, 샌드박스가 없는 카드(Claude, YOLO)가 O 상태 폴더 밖의 파일(터미널 세션 파일 M3, 완료 문서의 일회용 값 M4, 샌드박스 점검 기록 M8, PATH의 `gh` M2)을 직접 고치는 것은 막지 못한다. ADR-0014의 한계와 같다.
- 작업 개체 조회가 막힌 샌드박스(AppContainer) 안에서는 작업 개체를 보지 못해 환경 변수만으로 판정한다.
- 가드 훅은 CLI가 넘긴 환경에서 돌아 작업 개체 판정을 쓰지 않는다.
- H4: `guard.yml`은 여전히 PR의 코드(`o scan` 포함)를 빌드해 돌린다. 워크플로가 아닌 검사 코드 자체의 변경은 사람의 검토에 맡긴다. 대표의 브랜치에 다른 사람이 push한 뒤 대표가 PR을 열면 §7의 검사를 지난다.
- `merge-guard.yml`은 여전히 사후 경보다(M11). OpenCode의 권한 설정 의미는 설치 버전에 달려 있다(M9).

## 결과

- Codex 카드는 `o msg`를 우편함으로만 쓴다. 실행기가 죽으면 답이 오지 않아 75초 뒤 실패한다.
- 팀원이 워크플로를 바꾸려면 대표에게 PR을 열어 달라고 한다.
- `pull_request_target` 전환이 base 브랜치에 들어가기 전의 PR(이 변경을 담은 PR 포함)에는 `check`가 돌지 않는다. 대표가 병합한다.
- 대표가 할 일: `release` 환경 만들기(브랜치 `test_1` 제한)와 서명 비밀 옮기기.

## 확인하지 못한 것

- 실제 Codex 샌드박스에서의 우편함 왕복, 실제 앱 창에서의 이동 제한과 총괄·터미널 작업 개체.
- 워크플로(증명 생성, `pull_request_target`, `release` 환경): GitHub Actions가 돌지 않아 실행해 보지 못했다. 증명이 붙은 릴리스가 아직 없어 `gh attestation verify`의 성공 경로도 실제로 보지 못했다.
