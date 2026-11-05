# ADR-0030: macOS 작업별 coalition과 정확한 자손 종료 증명

상태: 구현·회귀 검증, 2026-11-05. 실제 공급자 검증은 별도 실기 기록에 남긴다.

## 문제

macOS의 NOTE_TRACK/NOTE_CHILD는 지원되지 않는다. 부모 PID·프로세스 그룹의 사후 조회는 이중 fork·setsid·재부모화를 놓친다. 첫 구현은 suspended spawn 앞에 kqueue를 붙이고 fork가 한 번이라도 발생하면 종료 불명으로 남겼다. 이 방식은 안전하게 거부하지만 실제 CLI가 명령을 실행한 뒤 다음 작업을 막는다.

## 결정

- 카드와 총괄은 macos_worker의 공통 실행 경로를 쓴다. 사용자 GUI launchd 도메인에 임시 작업을 등록하고, 동봉된 `o --o-mac-worker`만 시작한다. KeepAlive/RunAtLoad는 false이며, 시작은 명시적으로 kickstart한다. 관리자 권한·상시 LaunchAgent 설치가 필요하지 않다.
- launchd가 만든 resource coalition이 호출자와 다르고 활성 작업이 도우미 하나뿐임을 양쪽에서 검사한다. 필요한 macOS API가 없거나 격리/계수를 확인할 수 없으면 공급자를 시작하지 않는다. headless GUI 도메인이 없는 환경도 명확한 시작 실패다.
- 0700 임시 폴더의 Unix 소켓으로 실행 파일·인수·cwd·정확한 환경을 전달한다. plist에는 도우미와 소켓 경로만 기록한다. 네 연결 모두 uid/PID를 검사한다. 인증 환경이나 프롬프트를 plist/명령줄에 옮기지 않는다.
- 도우미는 native posix_spawn START_SUSPENDED/CLOEXEC_DEFAULT로 직접 자식을 만들고 coalition 상속을 확인한다. 부모가 시작 기록·중지 직렬화를 마친 뒤에만 resume한다. 공급자의 인수는 셸을 거치지 않는다.
- 직접 자식 종료, 중지, 소켓 단절 시 같은 coalition의 모든 자손을 정리한다. 조회는 종료 대상 발견에만 사용한다. 각 PID의 세대(pidversion)를 먼저 확보하고 coalition을 확인한 뒤 `proc_signal_with_audittoken`으로 신호를 보낸다. PID 재사용은 커널이 거부한다. 다른 작업·호출자·도우미 자신은 종료 대상이 아니다.
- 종료 **증명**은 PID 목록이 아니라 `coalition_info_resource_usage`의 `tasks_started - tasks_exited == 1`이다. 도우미만 남은 뒤 직접 자식의 종료 상태를 전달하고, 부모도 같은 커널 계수를 확인한 뒤 예약/총괄 표식을 해제한다. 도우미는 그동안 살아 있다. 셸 출력 파이프의 닫힘만으로 성공을 판정하지 않는다.
- 도우미의 출력 중계는 완료 증명을 막지 않는다. 부모가 출력을 읽지 않아도 중지 판정은 끝난다. 부모 EOF 시 도우미가 자손을 정리하고 자기 임시 launchd 등록을 해제한다. 정상 소유자도 Drop에서 정리한다.
- API/통신/계수 오류나 제한 시간 초과는 종료 불명이다. 기록된 오래된 PID로 복구 종료하지 않는다. 도우미 자체의 비정상 종료 등 증명이 없는 예약은 기존 보수적 복구 규칙을 유지한다.
- 파일 자원 별칭은 기존 CoreFoundation 정규 분해·볼륨 대소문자 정책 비교를 유지한다.

## 근거와 범위

Apple XNU의 resource coalition은 fork/exec/setsid 후에도 작업 소속과 시작/종료 누적 수를 유지한다. 별도 coalition 생성은 launchd에 맡긴다. 해당 API 일부는 공개 안정 ABI가 아니므로 런타임 심볼·구조 크기·독립성·계수 확인에 실패하면 실행을 거부한다. 다른 macOS 버전/Intel에서 동등한 지원을 추정하지 않는다. 같은 사용자 권한의 악의적인 프로세스나 특권 프로그램의 고의적인 이탈을 막는 보안 샌드박스는 아니다. 공급자 자체의 기존 sandbox/승인 정책은 변경하지 않는다.

- [Apple coalition 구조](https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/mach/coalition.h)
- [Apple coalition syscall](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/sys_coalition.c)
- [Apple proc_info](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/proc_info.c)
- 로컬 macOS SDK libproc.h의 audit-token 신호 API, launchd.plist(5).

## 검증

- 제품 카드/총괄 정상·중지·setsid 분리 자손·출력 파이프 상속: 자손 PID+시작 시각으로 실제 종료를 추가 확인하고 예약 해제를 검사한다.
- 환경 교체·인수·cwd, 시작 전 중지·없는 실행 파일, 동시 작업/무관한 프로세스 보존, 소유자 강제 종료.
- 같은 PID의 잘못된 세대 토큰은 ESRCH, 올바른 토큰은 성공. 공유 coalition 거부.
- 실제 AI 명령 실행·연속 작업·설치 앱 결과: docs/macos-verification-2026-11-05.md.
