# 보안 신고

project_O의 보안 문제는 공개 이슈로 올리지 말고 비공개로 알려 주세요.

## 신고 방법

저장소의 **Security** 탭 → **Report a vulnerability**(GitHub 비공개 취약점 신고)로 보낸다. 신고 내용은 신고자와 관리자만 본다.

적어 주면 좋은 것:

- 영향받는 버전(설정 → 정보의 커밋 SHA 또는 릴리스 태그)과 Windows 버전
- 재현 순서와 기대한 동작, 실제 동작
- 가능하면 최소 재현 계획(`plan.json`)이나 명령. 실제 계정 정보·토큰·개인 경로는 넣지 않는다

받은 신고는 7일 안에 확인 답장을 하고, 고치는 일정은 확인 뒤 알린다. 고친 버전이 나오기 전에는 내용을 공개하지 않기를 부탁한다. 고친 뒤 원하면 신고자를 감사 문구에 적는다.

## 지원 버전

| 버전 | 보안 수정 |
|---|---|
| `main`의 최신 정식 릴리스 | 지원 |
| `test_1-*` 최신 프리릴리스 | 지원 |
| 그 이전 빌드 | 최신으로 업데이트 후 다시 확인 |

## 범위

신고 대상으로 보는 것:

- O 가드(`o hook`, `o scan`)나 O 정책(`crates/o-core/src/policy.rs`)을 우회해 막아야 할 동작이 통과하는 경우 — 인증 파일·토큰 접근, 공급자 서버 직접 호출, 권한 확인을 끄는 옵션, 보호 브랜치 커밋, 카드 상한 초과 등
- 기본 모드(YOLO 꺼짐)에서 사람의 승인 없이 총괄이 파일을 고치거나 명령을 실행하는 경우
- 카드나 외부 입력(카드 메시지, 첨부, 실행 기록)이 총괄의 승인·계획 실행을 사칭하는 경우
- 업데이트 다운로드의 검증(파일 이름·크기·SHA-256 digest)을 건너뛰게 하는 경우
- 앱이 로그인 정보·토큰을 읽거나 기록·전송하는 경우

대상이 아닌 것 (알려진 한계, [README](README.md#보안-모델-요약)):

- 같은 Windows 사용자 권한으로 도는 프로그램이 O의 표식을 지우거나 별도 프로세스로 우회하는 것
- YOLO를 켠 상태에서 에이전트가 승인 없이 파일과 명령을 다루는 것 (YOLO의 의도된 동작)
- 각 공급자 CLI(Claude Code, Codex 등) 자체의 취약점 — 해당 공급자에 신고한다

## 구현 경계의 미해결 항목

[ADR-0046](docs/decisions/0046-security-remediation.md)의 보완은 OS 사용자 격리가 아니다. 같은 사용자 권한에서 승인 결정·실행 계획/모드·터미널·이벤트 파일을 직접 바꾸는 문제는 아직 해결하지 못했다. 실행 간 원시 로그 읽기와 Unix 상태 폴더의 다른 사용자 읽기 제한도 보장하지 않는다. 개별 발견의 해결/반증/미해결 구분은 [감사 조치표](security-audit/remediation.md)에 기록한다.

## English

Please do not open public issues for security problems. Use the repository's **Security → Report a vulnerability** (GitHub private vulnerability reporting). Include the affected version (commit SHA or release tag), Windows version and reproduction steps, without real credentials. We aim to acknowledge reports within 7 days. Bypasses of the O guard or policy limits, unapproved lead edits in the default mode, impersonation of approvals, and update-verification bypasses are in scope; the documented limits above and bugs in the providers' own CLIs are not.
