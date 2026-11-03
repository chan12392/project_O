# ADR-0023: Codex 총괄의 app-server와 독립 터미널

- 상태: 적용
- 날짜: 2026-11-03
- 관련: ADR-0008, ADR-0014, ADR-0016, ADR-0020

일반 모드의 Codex 총괄은 공식 `codex app-server`의 stdio 프로토콜로 한 턴을 시작하거나 기존 스레드를 재개한다. 프로젝트 훅은 기존처럼 끄고 스레드와 턴에 읽기 전용 샌드박스를 지정한다. CLI의 본인 로그인을 사용하며 O는 API 키나 로그인 정보를 전달하지 않는다. 세션이 턴 시작을 확인한 뒤에만 메시지 전달을 기록한다. 완료된 메시지·도구 단계·사용량은 기존 총괄 이벤트와 계획 검사에 연결한다. 중지와 시간 제한은 기존 프로세스 관리 경로를 따른다.

진행 중인 답변은 임시 미리보기로 표시하고 완료되면 저장된 답변으로 바꾼다. 계획과 설계 내용이 중간에 노출되지 않도록 첫 코드 펜스 뒤는 완료 전까지 표시하지 않는다. 스레드와 턴에는 승인 정책 `never`와 승인 검토자 `user`를 지정해 자동 검토가 먼저 승인하지 못하게 한다. 그래도 오는 명령·파일 변경 승인 요청에는 공식 `decline`을 응답하고 진행 상태에 표시한다. 권한·MCP 입력·사용자 질문 요청은 각 응답 형식으로 거절하고(권한 없음, `decline`, 빈 답), 그 외 요청은 JSON-RPC 오류로 거절한 채 턴을 이어 간다. 완료된 턴은 app-server 종료가 늦어도 프로세스만 끝내고 결과를 유지한다. 턴을 시작하기 전에 CLI가 `app-server`를 지원하는지 `codex app-server --help`로 한 번 확인하고(셸 없이, 창 없이, 10초 제한, CLI 경로별로 결과를 기억), 지원하지 않으면 이전의 읽기 전용 실행 `codex exec -s read-only --disable hooks`(재개는 `exec resume <id>`)로 총괄을 돌린다. 두 방식의 세션 ID는 같은 Codex 대화 ID라 CLI가 바뀌어도 이어서 재개한다. 지원한다고 확인된 app-server가 초기화 전에 실패하면 필요한 Codex 버전을 알리고 그 턴을 끝낸다. 앱에서 승인·질문을 주고받는 경로는 아직 없다. YOLO 총괄과 작업카드는 기존 CLI 경로를 유지한다. app-server의 출력과 세션 재개는 설치된 CLI 버전의 프로토콜에 의존한다.

OpenAI의 [서드파티 app-server 안내](https://developers.openai.com/siwc/token-sharing-open-source/codex-app-server)는 앱별 OAuth 동의 경로를 설명한다. 로컬 CLI의 기존 로그인만 쓰는 이번 방식이 O의 배포에도 허용되는지는 별도 확인한다.

Antigravity의 실행·자동 조회는 ADR-0020대로 계속 차단한다. 설정 화면의 터미널 버튼은 빈 독립 PowerShell만 연다. 사용자가 직접 폴더를 선택하고 `agy`를 입력하며, O는 agy 프로세스나 대화를 제어하지 않는다. Windows 외에는 터미널을 직접 열도록 안내한다.

Claude Agent SDK의 구독 인증은 [구독 안내](https://support.claude.com/en/articles/15036540-use-the-claude-agent-sdk-with-your-claude-plan)와 [제품 개발자 인증 조건](https://code.claude.com/docs/en/legal-and-compliance)의 적용 범위를 확인한 뒤 별도로 결정한다. 기존 구독 사용을 API 과금으로 자동 변경하지 않는다. Devin ACP는 읽기 전용 권한과 재개가 확인될 때까지 ADR-0016의 카드 전용 상태를 유지한다.
