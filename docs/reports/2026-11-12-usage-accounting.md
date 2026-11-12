# 사용량 정산·모델 목록·구독 한도 전환 구현 검사

분리 브랜치 `younchan-usage-models`. 실제 계정 설정, 설치 앱, 과거 실행 기록은 변경하지 않았다.

## 자동 검사

- Claude 전체 모델 합계·캐시 쓰기·실패 결과·부분 메시지·동일 메시지 중복·프로필별 재개 기준값·DB 기록 실패와 독립적인 정산을 검사했다.
- Codex 모든 깊이의 부모 연결·상속 누적값·중복 보고·다른 루트 제외·시간 범위·늦은 하위 기록·app-server delta를 검사했다. 설치 앱 시험의 부모 63,780 + 하위 64,369 = 128,149와 전체 192,991을 비식별 fixture로 재현했다.
- Rust와 TypeScript가 같은 정산 fixture를 읽어 최신 revision 교체, 같은 revision의 충돌, 다른 카드 대체 거부, 누락 기준값, 알 수 없는 이벤트 제외 후 안정적인 참조를 검사했다.
- 보정 미리보기가 저널·이벤트·DB를 만들거나 바꾸지 않는지, 저널만 남은 중단을 복구하고 다시 적용해도 중복되지 않는지 검사했다.
- 모델 캐시는 프로필·프로젝트를 분리하고 오래된 목록과 지원 effort를 검사했다.
- 자동 전환 테스트의 Claude/Codex 실행 파일은 테스트 바이너리의 복사본이다. 상태·모델 메타데이터와 작업 스트림만 응답하며 실제 CLI·서버·모델을 부르지 않는다. 일반/격리 작업 폴더, OFF, 실행 직전 OFF, 대상의 재차 한도, 미지원 모델, 불명확한 외부 작업, 총괄의 시작 전/실행 중 구독 한도를 검사한다.
- 테스트용 작업 폴더와 결과만 사용하며 실제 구독 소진, Claude 실행, 유료 모델 호출은 하지 않았다.

실행한 검사: `cargo test -p o-core --lib`, 영향받은 runner·lead·lead_worker·lead_usage·usage·context·snapshot·usage_accounting·fallback_runner 통합 테스트, 새 usage/models 단위 테스트, `cargo clippy -p o-core -p o-cli --all-targets -- -D warnings`, `cargo check -p project-o`, 루트/앱 `npm run check`, 루트 `npm test`(203개), 앱 `npm run build`, `cargo run -p o-cli -- scan --all`(599개 파일). 단위 테스트의 기존 Windows 전용 수동 검사 1개와 lead_worker의 기존 수동 검사 1개는 ignored 상태였다. 앱 빌드는 통과했고 큰 번들 및 settings 모듈의 정적/동적 import 혼용 경고가 남았다.

## 화면 확인

`beakho-design`에서 선택한 Impeccable 지침을 적용하고 기존 SettingsRow/Switch·필드·버튼 토큰을 재사용했다. Ego Lite의 `?source=demo`에서 기본 OFF, 대상 미선택 오류, 사용자 지정 모델 저장, ON/OFF, 화면 이동 후 저장 유지, 모델 입력에서 저장 버튼으로 키보드 이동, 1280px 화면의 가로 넘침 없음을 확인했다. 조회 중·실패·stale·버전 미확인 상태는 독립적으로 표시한다. 데모는 실제 설정 API를 호출하지 않는다.

기존 Vite 개발 플러그인이 데모에서도 `_lead-watch`를 시작하는 점을 확인해 서버와 감시 프로세스를 종료했다. 해당 시간 이후 실제 상태 폴더에 새 총괄·작업 질문·세션·사용량 이벤트가 0개임을 확인했다. 다음 데모 검사는 빌드한 정적 `vite preview`를 사용해 이 감시 경로도 제외한다.

## 남은 실제 환경 확인

- 실제 Claude 버전의 초기화 메타데이터와 구독 한도 전환은 호출하지 않았다. 실제 설치 앱의 Tauri 화면과 실제 CLI 조합 검증은 통합 뒤 별도 실행한다.
- 옛 총괄 기록에 턴별 원본 경계가 없으면 기존 수치와 불완전 사유를 유지한다. Codex의 전체 자손 목록은 공식 형식으로 완전성을 증명할 수 없어 부분 집계다.
- 원래 `usage/thread_total.rs`의 영구 삭제는 로컬 훅이 차단했다. 재시도하지 않았으며 모듈 참조만 제거했다. 현재 정산 경로에서는 컴파일·호출되지 않는다.
