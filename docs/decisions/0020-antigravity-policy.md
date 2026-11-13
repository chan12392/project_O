# ADR-0020: Antigravity 실행 보류

**개정 1 (2026-11-11):** 작업 에이전트 설정과 범위는 [ADR-0042 §3](0042-terminal-agents.md#3-antigravity)을 따른다. 창 없는 실행·예약과 자동 조회의 보류는 유지한다.

- 상태: 적용
- 날짜: 2026-11-03

Google의 [Antigravity FAQ](https://antigravity.google/docs/faq)는 소비자 플랜에서 `agy`를 제3자 앱 안에 감싸 실행하는 방식을 지원하지 않는다고 밝힌다. [추가 약관](https://antigravity.google/terms)은 일부 기업 계약에 별도 조건이 적용될 수 있다고 설명한다. Project O는 계정 유형이나 계약을 확인할 수 없다.

따라서 계정 보호를 위해 총괄·카드·예약의 Antigravity 실행을 중단한다. 자동 로그인 상태·모델·사용량 조회도 CLI를 실행하지 않는다. 기존 기록은 보존하며, 공식 허용 범위와 계정 구분 방법을 확인한 후 다시 검토한다. Claude와 Codex 경로는 그대로 유지한다.
