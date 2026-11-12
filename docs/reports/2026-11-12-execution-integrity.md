# 승인 계획·모드 전달 검증 — 2026-11-12

담당 브랜치 `younchan-execution-integrity`, 기준 `565e870`. statefs F3/F6의 파일 재독 경로를 없애고 부모가 검사한 입력을 실제 자식 실행까지 연결했다. 결정은 [ADR-0049](../decisions/0049-approved-execution-input.md)에 기록했다. OS 에이전트 격리와 상태 폴더 전체의 무결성이 완료됐다는 뜻은 아니다.

## 구현

- `RunInput`에 계획·프로필·컨텍스트·YOLO·예약·격리·후속 실행 정보를 잡는다. 일반 CLI, 총괄 분배, 예약, PR 검토, 후속 작업의 시작 콜백과 자식 실행기에 전달한다.
- 부모와 자식이 같은 실행 파일인지 OS가 제공한 연결 상대 PID로 확인한다. 일반 셸 파이프나 저장 파일은 승인 출처가 아니다. 내부 명령 직접 실행은 입력 검증을 통과해야 한다. 전달 실패는 오류이며 파일 실행으로 돌아가지 않는다.
- UI가 가진 원래 계획을 Tauri/dev/CLI/core까지 전달한다. 최신 계획, 프로젝트, 화면의 YOLO 모드가 맞지 않으면 실행하지 않는다. 계획 요약에서 payload를 재구성하지 않아 resource/model/effort도 보존한다.
- 완료된 후속 작업의 세션·폴더를 잡고, 대기 중 후속 작업의 조상 계획·모드·참조를 잡는다. 시작 직전 폴더 검증, 세션 잠금, 중지·대기·한도 전환을 유지한다. 기존 기록 형식은 바꾸지 않았다.
- PR 검토의 `context=false`는 검토 생성 함수가 반환하는 입력과 저장 표식에 한 번에 적용한다.

## 확인한 동작

모든 프로바이더 실행은 임시 상태 폴더와 가짜 CLI/Spawner를 사용했다. 실제 유료 모델 호출, 설치 앱 교체, 실제 O 운영 상태 수정은 하지 않았다.

| 검사 | 확인 결과 |
|---|---|
| 새 CLI `execution_integrity` | 실제 Windows 부모→자식 파이프 전달. 전달 전에 저장 계획의 provider/model/task와 프로필·YOLO·컨텍스트·예약·격리 표식을 변조해도 원래 task/model/effort/프로필/작업 폴더/mode 이벤트로 실행됨 |
| 내부 명령 거부 | 다른 실행 파일에서 보낸 JSON으로 `_run`, `_run-approved`, `_followup`, `_followup-approved` 호출 거부. 입력 없는 `_run`도 거부하고 실행 기록을 만들지 않음 |
| 사람 CLI·화면 승인 | 실제 `o dispatch`→가짜 프로바이더 성공. 화면 계획 불일치·YOLO 변경·대화 프로젝트 불일치 거부. 맞는 계획은 원래 payload와 모드로 전달 |
| CLI fallback_dispatch | 11개 정상·거부·분배·자식 재검사 시나리오 통과 |
| core runner | 43개 통과. 완료·실행 중 후속 작업, 대기 취소, 세션 직렬화, 실패 처리 포함. 승인 후 후속 참조·조상 계획·새 계획·YOLO 변조에도 잡아 둔 세션·폴더 사용 |
| core lead | 54개 통과. 예약 입력을 잡은 뒤 저장 계획·모드 표식을 바꿔도 승인된 예약 실행을 유지하는 회귀 포함 |
| 기타 core | context 11, dead_runner 6, detached 1, dispatch_isolation 4, lead_recovery 16, policy 28, review 17, yolo 3 통과. fallback_runner 카드 7·총괄 2 시나리오 통과 |
| TypeScript | 전체 263개 통과 후 관련 화면/어댑터/dev 검사 33개 통과. 최종 fixture 타입 수정 후 lead/승인 어댑터 28개와 루트 타입 검사 통과. 앱 타입 검사 통과 |
| 정적 검사 | `cargo clippy -p o-core -p o-cli --all-targets -- -D warnings`, `cargo check -p project-o`, `cargo run -p o-cli -- scan --all` 통과 (새 파일을 stage한 뒤 정책 검사 631개 파일) |

검사 중 예전 테스트가 파일 표식을 실행 설정으로 쓰던 부분은 새 입력 API를 사용하도록 옮겼다. 가짜 CLI의 stdin/이벤트 계약을 잘못 가정한 검사와 새 TypeScript fixture의 literal 타입을 고친 뒤 통과를 확인했다. 이전 실패가 남은 로그 대신 마지막 재실행 결과를 기준으로 위 표를 작성했다.

## 화면 확인

`beakho-design`이 선택한 Impeccable의 작업 흐름과 harden/craft-floor 지침을 적용했다. 기존 PlanCard·버튼·색상 토큰을 사용했다. Ego Lite 전용 taskspace에서 실제 PlanCard와 가짜 backend를 연결해 다음을 확인했다.

- 모드 미확인 시 버튼 비활성화, 모드 확인 후 활성화, 정상/YOLO 상태 표시.
- 키보드 Enter로 표시된 계획 전체와 프로필·모드를 전달하며, 처리 중 버튼이 잠김.
- 계획 불일치 오류의 alert 표시, 처리 후 키보드 초점 유지.
- 넓은 화면과 430px 화면에서 가로 넘침 없음. 상태 문구의 계산된 대비 약 7.90:1.

캡처는 담당 워크트리의 `target/integrity-wide.png`, `target/integrity-narrow.png`에 있다. liveRuns 플러그인을 넣지 않은 fixture 전용 서버를 사용했고 확인 후 서버와 taskspace를 종료했다. 실제 설치 앱을 대상으로 한 UI 검증은 아니다.

## 보안 작업자와 합칠 접점

| 파일 | 필요한 변경 |
|---|---|
| `crates/o-core/src/sys/proc.rs` | 기존 숨김·분리 실행 처리는 유지하고, stdin/stdout을 보존하는 `pub(crate) spawn_detached_with_stdio`를 분리했다. 기존 `spawn_detached`는 stdio를 null로 둔 뒤 이 함수를 부른다. 새 OS 격리 로직은 두 호출 경로에 공통 적용해야 한다. |
| `crates/o-core/src/run/runner/card.rs` | `Start.followup: Option<(&str, &Target)>`를 추가했다. 저장 참조 재독 대신 부모가 잡은 Target을 사용한다. provider/model/project/profile 일치 검사와 실행 직전 폴더 검사는 유지했다. 구독 한도 전환으로 새 provider를 시작하는 재귀 호출은 `followup: None`이다. |
| `app/src-tauri/src/main.rs` | 앱 자신이 `_run-approved`/`_followup-approved` 자식을 맡는다. 창 초기화 전 내부 실행 및 기존 Windows/macOS worker helper를 처리한다. OS 보안 작업에서 실행기 프로세스와 카드 worker를 구분할 때 이 앱 자식 경로도 포함해야 한다. |

`lead/turn_process.rs`, `lead/approvals.rs`, `term/*`는 수정하지 않았다. `review::create_run`은 저장한 검토의 `RunInput`을 반환하고, `context::prepare`는 명시적인 enabled 인자를 받는다. 관련 호출자와 테스트를 함께 바꿨다.

## 남은 검증과 경계

- Linux·macOS 컴파일/실행과 설치 앱의 실제 Tauri 자식 기동은 미검증이다. macOS는 Apple 소스를 대조해 부모 확인까지 소켓 연결을 유지하도록 했지만 실기 확인을 대신하지 않는다.
- 별도 보안 작업자의 OS 격리·상태 접근권 변경을 합친 뒤 입력 파이프 상속, 앱 자식 실행, worker 핸들 소유자를 함께 확인해야 한다. 이 담당 브랜치는 그 변경이 없는 상태에서 Windows 입력 전달을 검증했다.
- 부모 프로세스·실행 파일·핸들 자체의 침해, 승인 전 제안 파일 변조, 실행 중 새로 생기는 이벤트·중지·예산 등 모든 상태 파일의 무결성은 이 변경만으로 보장하지 않는다. 대기 중 후속 작업의 최종 세션·완료·전환 이벤트도 살아 있는 원본 실행에서 계속 읽는다.
- 인라인 Python 파이프 조사와 Impeccable 컨텍스트 런처는 로컬 훅이 동적/불투명 명령으로 차단해 재시도하지 않았다. 실제 Rust 통합 검사와 이미 읽은 스킬 지침으로 허용된 작업을 진행했다.
- 구현·fixture·경로 조회·macOS 소켓 생명주기의 자체 발견 실수 4건은 교훈으로 저장하고 Horang에 색인했다. 원문 대화나 비밀값은 기록하지 않았다.
