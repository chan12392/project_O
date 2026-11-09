# ADR-0021: test_1_hotfix 핫픽스 차선

- 상태: 채택
- 날짜: 2026-11-03
- 관련: ADR-0003(test_1·test_2 흐름)

## 배경

`test_1`은 설치 파일 프리릴리스와 앱 업데이트가 따라가는 배포 채널이다. 배포된 빌드에서 고칠 것이 나와도 `test_2`에는 아직 확인하지 않은 작업이 쌓여 있을 수 있어, 평소 흐름(`test_2` → `test_1`)으로는 급한 수정만 따로 내보낼 수 없다.

## 선택지

1. 평소 흐름만 쓴다 — `test_2`의 미확인 작업이 수정과 함께 배포된다.
2. `test_1`을 핫픽스 브랜치로 바꾼다 — 배포 채널·업데이트 태그·CI를 모두 `main`으로 옮겨야 한다.
3. `test_1` 앞에 핫픽스 전용 보호 브랜치 `test_1_hotfix`를 둔다.

## 결정

3을 고른다.

```
평소:   개인·작업 브랜치 ──PR──▶ test_2 ─────────┐
핫픽스: 개인·작업 브랜치 ──PR──▶ test_1_hotfix ──┴─PR──▶ test_1 ──PR──▶ main
```

- `test_1_hotfix`는 보호 브랜치다. 직접 커밋·push하지 않고, 대표만 병합한다(`o hook`, `merge-guard.yml`).
- `test_1_hotfix`로는 개인·작업 브랜치와 `test_1` 동기화만 올릴 수 있다. `test_1`로는 `test_2`와 `test_1_hotfix`만 올릴 수 있다(`branch-flow.yml`).
- 핫픽스 작업 브랜치는 `test_1`에서 갈라 `<개인>-<주제>`로 만든다. `test_1`에 병합된 뒤에는 평소처럼 개인 브랜치에 `test_1`을 병합하고 `test_2`로 올려 되맞춘다.
- 함께: PR의 base를 확인하지 못한 병합은 대표 로그인이어도 막는다(#47).

## 영향

- 고정 브랜치가 6개, 보호 브랜치가 4개가 된다. `READ_ONLY_BRANCHES`(`crates/o-guard/src/rules.rs`), `branch-flow.yml`, `merge-guard.yml`, `guard.yml`, AGENTS.md를 함께 고쳤다.
- 배포 채널(`release.yml`, 앱 업데이트)은 그대로 `test_1`이다.
