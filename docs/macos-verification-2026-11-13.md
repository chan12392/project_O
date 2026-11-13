# macOS 최신 통합본 검사 — 2026-11-13

## 기준과 수정

- 기준: `test_2` `10386c2a748012daf759cc604abaa733de98039e` (PR120 병합본).
- 은미 개인 브랜치에서 동기화 후 검사. 다른 사람의 열린 PR은 병합하지 않았다.
- #121을 macOS에서 재현: `dream::tests::documents_are_found_by_their_tags`가 빈 결과와 shop 문서의 불일치로 실패했다.
- 테스트의 경로 구분자 정규화와 OS별 대소문자 계약을 분리했다. Windows는 소문자 드라이브 조회도 같은 문서를 찾고, 비 Windows는 다른 대소문자 프로젝트를 섞지 않는 것을 검사한다. 제품 코드와 프로젝트 격리 규칙은 변경하지 않았다.

- 전체 검사에서 예약 시험 하나가 `table resource_leases already exists`로 간헐 실패했다. 동일 프로세스에서 시간값만으로 임시 폴더 이름을 만들면 병렬 시험이 같은 폴더를 공유할 수 있다. 실패 실행에는 자료를 만드는 시험 9개에 폴더 8개만 남아 있었다. 프로세스 내 고유 순번을 이름에 추가하고 기존 폴더를 조용히 재사용하지 않도록 수정했다. 제품 저장 형식이나 보호 검사는 변경하지 않았다. 수정 후 예약 시험 10개 통과.

## 확인 결과

- 수정 전 해당 Wiki 테스트 실패 → 수정 후 Wiki 테스트 13개 통과.
- TypeScript 테스트 200개, 루트 타입 검사, 앱 타입 검사·화면 빌드 통과.
- `cargo clippy --workspace --all-targets -- -D warnings` 통과.
- `o scan --all`: 599개 파일 통과. npm 실행 의존성 라이선스 검사 통과.
- 최종 `TMPDIR=/private/tmp cargo test --workspace --no-fail-fast` 종료 코드 0. 표준 요약 기준 594개 통과·0개 실패·8개 제외, 별도 실행 방식의 lead/runner/sandbox 검사도 통과. 제외 항목은 Windows 전용 4개, 실제 Codex 수동 통합 2개, 성능 측정 1개, 설치 CLI 수동 점검 1개다.
- 수정 커밋: Wiki `5256903`, 예약 시험 `b73e44e`. 검토용 PR: https://github.com/chan12392/project_O/pull/125 (미병합).

## macOS 임시 경로 주의

macOS 기본 `TMPDIR`의 `/var`는 실제 `/private/var`를 가리킨다. 기본 환경에서 CLI context·첨부 검사 등이 논리 경로와 실제 경로 차이 또는 링크 경로 거부로 실패했다. `TMPDIR=/private/tmp`로 CLI context/lead 13개 검사를 다시 실행하면 통과한다. 링크 거부를 완화하지 않았으며 이것을 제품 수정으로 보고하지 않는다.

재현 가능한 전체 검사 명령:

```sh
TMPDIR=/private/tmp cargo test --workspace --no-fail-fast
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p o-cli -- scan --all
npm test
npm run check
npm run build --prefix app
npm run check:licenses
```

## 확인 범위

이 기록은 로컬 macOS 자동 검사다. 새 배포 앱 설치·사람의 실제 로그인·새 유료 AI 작업·Intel Mac·Windows 실기는 이번 기록의 통과 범위가 아니다. 11월 5일 실제 AI 검증을 최신 버전 실기로 재사용하지 않는다. Linux·Windows는 PR CI 결과로 별도 확인한다.

## 원본 대조

Discord 일반 채널의 11월 9일 회의 기록은 11월 말 QA/UI/Wiki 작업을 명시했다. 11월 13일 일정 변경은 다음 주로 수락됐지만 정확한 새 날짜는 확인되지 않았다. 윤찬-작업의 새 기능 기록과 빈 은미-작업 채널을 확인했다. 외부 메시지는 보내지 않았다.
