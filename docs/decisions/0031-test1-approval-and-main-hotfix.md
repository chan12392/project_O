# ADR-0031: test_1 승인 병합과 main_hotfix 핫픽스 차선

- 상태: 채택
- 날짜: 2026-11-06
- 관련: [ADR-0003](0003-team-flow-test1-test2.md)(test_2·test_1 흐름과 대표 병합), [ADR-0012](0012-remove-approval-waits.md)(승인 대기 없는 흐름), [ADR-0021](0021-test1-hotfix-branch.md)(test_1_hotfix 핫픽스 차선)

## 배경

- 저장소를 GitHub Pro로 올려 비공개 저장소에서도 규칙 집합(ruleset)을 쓸 수 있게 됐다. 지금까지는 무료 플랜이라 GitHub가 아무것도 막지 못해, 대표만 병합하는 규칙을 에이전트 훅·git 훅·사후 CI 경보로만 지켰다.
- 팀이 4명으로 늘었다. `test_2` → `test_1` 병합을 대표 한 사람만 할 수 있어, 확인이 끝난 작업도 대표가 볼 때까지 배포 채널(`test_1` 프리릴리스)에 들어가지 못한다.
- `main`은 정식 릴리스다. 정식 릴리스에서 고칠 것이 나와도 `test_1`에는 아직 정식으로 내보내지 않은 작업이 있을 수 있어, 평소 흐름(`test_1` → `main`)으로는 급한 수정만 따로 내보낼 수 없다. `test_1`에 대한 `test_1_hotfix`(ADR-0021)와 같은 문제다.

## 결정

```
평소:         개인·작업 브랜치 ──PR──▶ test_2 ──PR──▶ test_1 ──PR──▶ main
핫픽스:       개인·작업 브랜치 ──PR──▶ test_1_hotfix ──PR──▶ test_1
핫픽스(정식): 작업 브랜치 ──PR──▶ main_hotfix ──PR──▶ main
```

- GitHub 규칙 집합(대표는 저장소 관리자로서 둘 다 우회할 수 있다)
  - `main`: PR로만 바뀌고, CI `policy`·`check` 통과가 필요하며, 강제 push·삭제를 막는다. 병합은 여전히 대표만 한다.
  - `test_1`: 작성자가 아닌 사람 1명의 승인이 있어야 병합된다.
- `test_1` 병합은 다른 팀원 1명이 승인하면 누구나 한다. 대표는 승인 없이 병합할 수 있다. GitHub가 승인을 강제하므로 에이전트 훅(`o hook`)은 `test_1` 병합을 더 이상 막지 않고, `merge-guard.yml`도 `test_1`을 지켜보지 않는다.
- `test_1_hotfix`는 그대로다(대표만 병합, 승인 불필요).
- 새 보호 브랜치 `main_hotfix`를 둔다. 정식 릴리스의 급한 수정은 `main`에서 가른 `<개인>-<주제>` 작업 브랜치 → `main_hotfix` → `main`으로 올린다. `main_hotfix`와 `main` 병합은 대표만 한다. `main_hotfix`로는 개인·작업 브랜치와 `main` 동기화만, `main`으로는 `test_1`과 `main_hotfix`만 올릴 수 있다(`branch-flow.yml`).
- `main`에 들어간 수정은 개인 브랜치에 `main`을 병합해 `test_2`로 올려 되맞추고, `test_1`에도 넣는다(그 개인 브랜치 → `test_1_hotfix` → `test_1`, 또는 다음 `test_2` → `test_1` 릴리스).
- 릴리스: `test_1` push는 지금처럼 프리릴리스(`release.yml`, 바꾸지 않음), `main`은 대표가 게시하는 정식 릴리스다.

## 영향

- 보호 브랜치가 5개, 고정 브랜치가 8개가 된다. `READ_ONLY_BRANCHES`(`crates/o-guard/src/rules.rs`)에 `main_hotfix`를 더해 에이전트 훅·git 훅(`o scan`)·거울 동기화·O CLI 정책이 함께 보호한다.
- 에이전트 훅은 `test_1_hotfix`·`main_hotfix`·`main`에 대한 대표 외 병합만 막는다. `branch-flow.yml`, `merge-guard.yml`, `guard.yml`(릴리스형 PR의 중복 검사 건너뛰기에 `main_hotfix` 추가), AGENTS.md를 함께 고쳤다.
- 규칙 집합은 누가 병합하는지를 제한하지 않는다. 대표 전용 브랜치(`test_1_hotfix`·`main_hotfix`·`main`)를 사람이 웹 화면에서 병합하는 것은 여전히 사후 경보(`merge-guard.yml`)와 규칙으로 지킨다.
- `test_1` 승인은 ADR-0012의 "승인 대기 없음"의 예외다. 배포 채널로 가는 한 단계에만 사람 1명의 확인을 두어 대표 혼자 막히는 지점을 없앤다.

## 개정 1 (2026-11-09): 나머지 보호 브랜치 규칙 집합

공개 전환을 앞두고 규칙 집합이 없던 보호 브랜치에도 건다. 대표(저장소 관리자)는 모두 우회할 수 있다.

- `test_2`: PR로만 바뀌고(승인 불필요, merge commit만), CI `policy`·`check` 통과가 필요하며, 강제 push·삭제를 막는다. 병합은 지금처럼 팀원 누구나 한다.
- `test_1_hotfix`·`main_hotfix`: 위와 같고, 여기에 "업데이트 제한"을 더해 우회 권한이 있는 대표만 브랜치를 바꿀 수 있다. 팀원(쓰기 권한)은 PR을 열 수 있지만 병합 버튼은 GitHub가 막는다.
- `main`: 기존 규칙 집합에 같은 업데이트 제한을 더했다. 위 "영향"의 웹 화면 병합 공백이 대표 전용 브랜치 3개 모두에서 닫힌다. `merge-guard.yml`은 사후 경보로 그대로 둔다.
- 팀원이 아닌 사람은 쓰기 권한이 없어 어느 브랜치에도 push·병합할 수 없고, 포크에서 PR만 열 수 있다(`branch-flow.yml`이 포크 PR을 실패시킨다, CONTRIBUTING.md).
