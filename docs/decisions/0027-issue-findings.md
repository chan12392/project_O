# ADR-0027: GitHub 이슈와 O 발견 연결

GitHub 이슈가 장기 추적의 정본이다. O는 이슈를 생성·수정·닫거나 댓글을 달지 않는다. `o issues search owner/repo "검색어"`는 공식 gh로 열린 이슈와 닫힌 이슈를 검색해 JSON으로 돌려준다. 검색 결과는 후보이며 중복 여부를 자동 판정하지 않는다. 100개에 도달하면 `possiblyTruncated`가 참이므로 검색어를 좁힌다. gh 인증·네트워크 실패는 오류로 전파하며 빈 결과로 바꾸지 않는다.

`o issues record finding.json`은 로컬 O 상태 폴더 `issues.db`에 발견을 보존하고 ID를 돌려준다. `o issues list owner/repo`로 다시 볼 수 있다. 동일 저장소·실행·카드·SHA·요약의 중복 저장은 거부한다. 서로 다른 프로세스의 저장은 SQLite가 직렬화하고 잠금이 풀리지 않으면 오류를 돌려준다. 에이전트 카드는 이 명령으로 상태를 기록할 수 없으며 기존 팀 메시지 경계를 따른다.

입력 예시 (실제 실행과 전체 커밋 SHA, 증거 참조로 바꿔야 한다):

```json
{
  "repo": "owner/repo",
  "runId": "run-123",
  "cardId": "card-1",
  "sha": "0123456789012345678901234567890123456789",
  "summary": "증상 요약",
  "reproduction": "재현 단계와 관찰 결과",
  "evidence": ["실행 로그 또는 파일의 참조"],
  "classification": "unclassified",
  "issue": null
}
```

`classification`은 `unclassified`, `external`(외부 장애), `contention`(권한·자원 경합), `related`, `duplicate` 중 명시적으로 선택한다. 확실하지 않으면 `unclassified`로 둔다. `related`와 `duplicate`에는 `"issue": {"number": 123, "updatedAt": "검색 결과의 updatedAt"}`이 필수다. 저장 직전 같은 저장소·번호의 이슈를 다시 읽고 수정 시각이 다르면 거부한다. 이는 기록 시점의 관찰이며 이후에도 이슈가 바뀔 수 있으므로 실제 이슈를 다루기 전 다시 확인해야 한다. 오프라인 발견은 미분류 등으로 기록하고 나중에 `o issues classify <id> duplicate 123 <updatedAt>`으로 연결할 수 있다. 이슈 없는 분류는 `o issues classify <id> external`처럼 쓴다. 동시 변경은 거부하며 재조회 후 판단한다. 발견 내용 수정·삭제 기능은 없다.

이슈 제목과 다른 외부 텍스트는 비신뢰 데이터다. 이 경로는 에이전트 프롬프트에 이슈를 삽입하거나 명령·권한으로 해석하지 않는다. 사람은 검색 결과와 재현·증거를 확인해 분류하고, GitHub에서 별도로 변경 여부를 결정한다. 기록은 로컬 PC에만 남으므로 다른 PC에서 공유하려면 사람이 GitHub 정본에 필요한 내용을 직접 반영해야 한다.
