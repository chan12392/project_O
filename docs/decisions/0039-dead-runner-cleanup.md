# ADR-0039: 실행기가 죽은 실행의 자동 정리와 자원 예약 복구

- 상태: 채택
- 날짜: 2026-11-10
- 관련: [ADR-0022](0022-agent-session-continuations.md), [ADR-0025](0025-parallel-handoff.md), [ADR-0037](0037-parallel-cards-and-subagents.md)
- 개정 1 (2026-11-10): Codex 검토(커밋 2145dcb) 반영. 예약 복구 증거를 부모 pid 추적에서 카드의 이름 있는 Job Object로 바꾸고, 중단된 카드의 후속 세션 규칙과 읽기 쪽 보완을 더했다.
- 개정 2 (2026-11-10): Job 이름 부재를 종료 증거에서 제외했다. 별도 Windows watcher가 실행기 종료와 Job의 활성 프로세스 수 0을 확인한 뒤 해당 예약에만 `exit_confirmed`를 남긴다. 새 세션 후속 잠금도 기존 세션 ID 및 다른 실행·카드 조합과 충돌하지 않게 분리했다.

## 배경

2026-11-10에 실행 `20261109-123716-tent`가 `o status`에서 계속 `[running]`으로 보였다. 카드 `gui`는 `running`, `package`·`review`는 `queued`였다. 그런데 실행기(`o _run`, pid 11796)와 카드 CLI(작업 프로세스, pid 39008)는 이미 둘 다 없었다.

- **실행이 끝나지 않는다**: 실행기만 `run.end`를 쓴다. 실행기가 죽으면 `events.jsonl`은 열린 채로 남는다. `store::read_runs`는 후속 실행(`run.followup`)에 한해서만 메모리에서 `card.end`·`run.end`를 덧붙였다. 보통 실행은 `o status`·앱·종료 확인에서 계속 "실행 중"으로 보였다.
- **자원 예약이 풀리지 않는다**: `gui` 카드의 자원 예약(gui.py·__main__.py 쓰기, io.py·cleaning.py 읽기)은 `state: "unknown"`으로 남았다. `resource_leases`·`recover_resource`가 복구할 수 있다고 보는 경우는 둘뿐이다. 하나는 살아 있는 실행기가 `mark_resource_exited`로 남기는 `exit_confirmed`, 다른 하나는 시작하지 않은 예약이다. 죽은 실행기는 어느 쪽도 남기지 못하므로, 사람이 O 안에서 그 파일들을 풀 방법이 없었다.

## 선택지

1. 지금처럼 둔다. 사람이 상태 폴더를 직접 고쳐야 하고, 같은 파일을 쓰는 카드는 계속 기다린다.
2. 실행기가 살아 있지 않으면(`!is_alive`) 실행을 닫고 예약을 바로 푼다. pid 재사용이나 열 권한이 없는 프로세스를 "죽음"으로 잘못 읽을 수 있다. 그러면 아직 파일을 쓰는 프로세스가 있는데 다른 카드가 같은 파일을 받는다.
3. 강한 증거가 있을 때만 실행을 기록으로 닫는다. 예약은 증거가 갖춰지면 "복구 가능"으로만 바꾸고, 푸는 것은 사람이 한다. (대표 선택: "자동 정리 규칙 추가")

## 결정

3을 고른다.

### 1. 실행기가 죽은 실행을 기록으로 닫는다 (`store::close_dead_run`)

아래 조건을 모두 만족하는 실행에는 `events.jsonl`에 이벤트를 덧붙인다.

- 조건:
  - `run.end`가 없다.
  - `runner.json`이 있고 시작 시각(`started`)이 적혀 있다.
  - 그 실행기가 `proc::definitely_exited(pid, started)`로 확실히 끝났다. 즉 프로세스가 없거나, 끝났거나, 같은 pid의 생성 시각이 다르다.
  - `plan.json`을 읽을 수 있고 카드 목록이 있다. 읽지 못하면 닫지 않는다. 카드의 `card.end` 없이 `run.end`만 쓰는 일은 없다.
  - 그 실행의 살아 있는 카드 표식(`active_markers`)이 없다. 표식은 추가 잠금 안에서 다시 읽는다. 호출한 쪽이 표식을 읽은 뒤에 시작한 카드가 있어도 그 실행을 닫지 않는다.
- 덧붙이는 이벤트:
  - 계획의 카드 중 `card.end`가 없는 카드마다 `card.end` `failed`를 덧붙인다. 결과는 "실행기가 끝나 이 카드를 마치지 못했습니다(중단됨). 작업 폴더를 확인하고 다시 보내 주세요"이고 `exitCode`는 없다. 후속 실행은 기존 문구 "후속 실행이 중단됐습니다. 다시 보내 주세요"를 그대로 쓴다.
  - 마지막에 `run.end` `failed`를 덧붙인다.
- 중복 방지: 덧붙이기는 실행기와 같은 추가 잠금(`with_events`: 프로세스 안 뮤텍스와 `events.lock` 파일 잠금)을 쓴다. 잠금 안에서 이벤트를 다시 읽고, `run.end`가 이미 있으면 아무것도 쓰지 않는다. 그래서 두 읽기가 같은 실행을 두 번 닫지 않는다.
- 돌려주는 것: 잠금 안에서 읽은 그 실행의 전체 이벤트 목록이다. 이번에 덧붙인 줄도 들어 있다. 이번 호출이 닫았든, 다른 읽기가 먼저 닫았든, 살아 있는 카드 때문에 열어 두었든 같다. 부르는 곳은 앞서 읽은 이벤트 대신 이 목록을 보여 준다. 그래서 색인을 읽은 뒤 다른 읽기가 실행을 닫았거나 줄이 덧붙었어도 모든 읽기가 닫힌 상태를 보여 준다. 실행기가 살아 있을 수 있거나 계획을 읽지 못해 잠금까지 가지 않았으면 아무것도 돌려주지 않는다(`None`).
- 닫지 않는 경우:
  - `runner.json`이 없는 실행. 전달 직후 120초 시작 유예와 시작하지 못한 실행은 지금처럼 둔다.
  - 시작 시각을 모르는 실행. 후속 실행은 지금처럼 메모리에서만 끝난 것으로 보인다.
  - 계획을 읽지 못하는 실행.
- 부르는 곳: 실행을 화면·상태로 읽는 곳에서 부른다.
  - `store::read_runs`: `o runs`, 앱 실행 목록, 개발 서버 `/runs`.
  - `o status`.
  - `o events --follow`: 죽은 실행에서도 끝난다.
  - 앱 종료 확인(`quit_activity`).
- 비용: 이미 `run.end`가 있는 실행은 검사하지 않는다. 열린 실행만 `runner.json`과 프로세스 하나를 확인한다. 실행기가 끝난 실행만 잠금을 잡고 카드 표식을 다시 읽는다. 이벤트를 덧붙이면 색인(`o.db`)은 다음 갱신에서 그 줄을 그대로 읽는다.

### 2. 중단된 카드의 후속 지시는 새 세션으로 시작한다 (`followup::target`)

`close_dead_run`이 쓴 `card.end`는 정상 종료가 아니다. 카드 CLI의 세션이 중간에 끊긴 것이다. 그래서 후속 지시가 그 세션을 이어 가지 않는다.

- 카드의 마지막 `card.end`가 `failed`이고, `exitCode`가 없고, 결과가 위 두 문구(`store::RUNNER_STOPPED`·`FOLLOWUP_STOPPED`) 중 하나이면 중단된 카드로 본다.
- 중단된 카드의 후속 지시는 그 카드의 `card.session`을 쓰지 않는다. 앞선 후속 실행에서 이어받지도 않는다. 같은 작업 폴더에서 새 세션으로 시작한다(`Target.session == None`). O는 재개 인자(`--resume`, `resume`, `--session`)를 넣지 않고 `card.session`도 미리 쓰지 않는다. 새 세션의 ID는 보통 카드처럼 CLI가 알려 준다.
- 새 세션으로 시작한 후속 실행이 자기 세션을 알리지 못했으면, 그 실행의 다음 후속 지시도 새 세션으로 시작한다. 중단과 상관없이 세션을 알린 적이 없는 카드는 지금처럼 "이어갈 세션 ID가 없습니다"로 거부한다.
- 같은 원래 카드의 후속 실행이 동시에 둘 돌지 않게 하는 잠금은, 새 세션이면 `session-locks/fresh/`에 둔다. 원래 실행 ID와 카드 ID를 각각 SHA-256의 hex 문자열로 바꾼 뒤 구분자 `.`으로 이어 파일명을 만든다. 조합의 경계가 모호하지 않고 파일명 길이도 고정되며, 예전 `fresh-<run>-<card>`와 같은 실제 세션 ID에도 걸리지 않는다. 세션 재개의 기존 잠금 경로와 키는 바꾸지 않는다.
- `resource_held`·살아 있는 카드 표식 검사는 그대로다. `events.jsonl` 형식은 바꾸지 않는다.

### 3. 별도 watcher가 카드의 종료를 확인한다 (Windows만)

시작한 예약(`spawn_intent == true`)은 `exit_confirmed == true`이고 예약 소유자(실행기)가 확실히 끝났다는 기존 `owner_gone` 검사를 통과해야 `recoverable`이다. `resource_leases`는 이 증거로 상태를 표시하고, `recover_resource`는 트랜잭션 안에서 예약 행과 같은 증거를 다시 읽고 확인한다. 시작하지 않은 예약의 기존 복구 조건은 그대로다.

실행기가 죽었을 때 `exit_confirmed`를 남기는 역할은 Windows 전용 `run::job_watch`가 맡는다. `proc::job_gone`·Job 이름 부재는 종료 증거에서 뺀다. `OpenJobObjectW`의 `ERROR_FILE_NOT_FOUND`도 종료 확인이 없으면 `unknown`이다. `resource_leases.job_name` 열은 watcher가 정확한 예약과 Job을 찾는 데 계속 쓴다.

Job 이름은 이렇게 만든다.

- Windows 카드는 이미 실행기의 Job Object(`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`) 안에서 돈다(`win_launch::Launch::spawn_in_named_job`). 이 Job에 `Global\project_O-job-<runId>-<cardId>-<임의 16바이트 hex>` 이름을 붙인다. 임의 값은 `BCryptGenRandom`으로 만든다.
- 그 이름으로 만들지 못하거나 이미 있는 이름이면(`ERROR_ALREADY_EXISTS`) 이름을 쓰지 않는다. 열린 핸들은 닫고 이름 없는 Job을 새로 만들며, 이름은 기록하지 않는다. 남의 Job을 받아 쓰지 않는다.
- `Local\`이 아니라 `Global\`을 쓴다. `Local\` 이름은 로그온 세션마다 따로 있으므로, 다른 로그온 세션에서도 같은 Job을 열 수 있게 한다. 열리지 않는다는 사실은 종료 증거로 쓰지 않는다.
- Job에는 빠져나가기 제한(`JOB_OBJECT_LIMIT_BREAKAWAY_OK`·`JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK`)을 켜지 않는다. 작업 프로세스가 띄운 프로세스는 Job을 떠날 수 없다. CLI가 만든 중첩 Job도 이 Job 안에 있다.
- 작업 프로세스가 그 Job에 들어 있는지 `IsProcessInJob`으로 확인한 뒤에만 이름을 넘긴다. 이름은 `link_slot`이 작업 프로세스 pid·생성 시각과 함께 `resource_leases.job_name`(새 열, 색인 스키마 8)에 기록한다.
- 총괄 턴과 짧은 명령(`proc::contain`)의 Job은 예약이 없으므로 이름을 붙이지 않는다.

watcher의 순서는 다음과 같다.

1. 작업 프로세스가 이름 있는 Job에 속하고 예약 행에 연결된 뒤, 실행기가 숨김 명령 `o _job-watch`를 `proc::spawn_detached`로 시작한다. watcher는 카드의 Job 밖에서 돌고 실행기의 표준 입출력 핸들을 상속하지 않는다. 전달하는 값은 O home, 예약 슬롯 ID, 실행 ID, 카드 ID, Job 이름, 작업 프로세스 pid·생성 시각, 실행기 pid·생성 시각뿐이다. 실행기는 watcher의 준비나 종료를 기다리지 않는다.
2. watcher는 Job을 `JOB_OBJECT_QUERY | JOB_OBJECT_TERMINATE` 권한으로 열고, 실행기를 동기화·조회 권한(`SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION`)으로 열어 생성 시각을 확인한다. 예약 행의 슬롯·실행·카드·소유자 pid/생성 시각·작업 프로세스 pid/생성 시각·Job 이름과 `spawn_intent == true`가 모두 맞아야 한다. 살아 있는 작업 프로세스는 pid로 열어 생성 시각과 `IsProcessInJob`을 확인한다. 작업 프로세스가 확실히 이미 끝난 경우에는 열린 Job과 정확히 일치하는 예약 행으로 확인을 이어간다. 생성 시각 불일치, 다른 Job 소속, 조회 실패 등으로 대상을 확인하지 못하면 핸들을 닫고 아무것도 기록하지 않는다.
3. 처음 검증한 작업 프로세스 핸들을 유지한 채 실행기 핸들을 기다려 실행기 종료를 확인한다. 이 단계는 폴링하지 않는다.
4. `JobObjectBasicProcessIdList`를 읽고(버퍼가 모자라면 키워 다시 읽는다), 각 pid를 동기화·조회 권한으로 열어 `IsProcessInJob`을 확인한 핸들을 보유한다. `OpenProcess`의 `ERROR_INVALID_PARAMETER`만 이미 사라진 프로세스로 처리하며, 다른 오류나 소속 확인 실패에는 기록하지 않고 끝낸다. `TerminateJobObject`로 카드 Job의 종료를 요청한 다음 목록을 다시 읽어 새로 보인 pid도 같은 방식으로 더한다.
5. `QueryInformationJobObject(JobObjectBasicAccountingInformation)`의 `ActiveProcesses == 0`을 확인할 때까지 폴링한다. 이어서 작업 프로세스와 수집한 후손의 모든 핸들을 `WaitForSingleObject(INFINITE)`로 기다린다. 어느 대기에도 완료 기한은 두지 않는다. 종료 요청 성공이나 활성 프로세스 수 0만으로는 확인을 남기지 않는다.
6. 모든 대기가 끝난 뒤에만 단일 UPDATE로 `exit_confirmed = 1`을 기록한다. 조건에는 슬롯 ID, 실행 ID, 카드 ID, 소유자 pid·생성 시각, 작업 프로세스 pid·생성 시각, Job 이름, `spawn_intent = 1`이 모두 들어가며 정확히 일치하는 예약 한 행만 바꾼다. 이미 해제되었거나 종료 확인된 예약에는 쓰지 않고, 일치하지 않는 행을 대신 갱신하거나 다시 만들지 않는다. 마지막으로 핸들을 닫고 끝난다.

Windows 실제 프로세스 검사에서 `ActiveProcesses == 0` 직후 작업 프로세스 핸들의 0ms 대기가 `WAIT_TIMEOUT`을 반환하는 경우를 확인했다. 그래서 회계 값 검사 뒤에도 핸들의 종료 신호를 따로 기다린다. 다만 생성 시점부터 모든 프로세스를 추적하는 완료 포트는 도입하지 않는다. 두 목록 사이에 스스로 끝나 목록에서 빠진 후손의 핸들은 수집하지 못할 수 있다. 이 판단의 근거는 [TerminateProcess](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-terminateprocess)의 미완료 I/O 완료·취소 전에는 프로세스가 끝나지 않는다는 계약과, [Job 회계 정보](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_accounting_information)의 종료된 프로세스가 빠지며 활성 수가 감소한다는 설명이다. 이에 따라 남는 틈은 종료 정리 중인 프로세스의 핸들 신호 시점으로 판단하지만, 관측하지 못한 모든 후손의 핸들 신호까지 직접 증명하는 것은 아니다.

watcher가 Job 핸들을 보유하므로 실행기 쪽 핸들만 닫아서는 마지막 핸들이 닫히지 않을 수 있다. `KillOnClose::drop`은 `CloseHandle` 전에 `TerminateJobObject`를 명시적으로 호출한다. 정상 종료·중지·시간 초과·오류 경로 모두 기존처럼 카드의 프로세스를 종료해야 한다. drop의 종료 요청 자체는 종료 완료 증거가 아니며, 정상 경로에서도 기존 종료 확인을 거쳐야 예약을 푼다.

실패할 때는 확인되지 않은 예약을 `unknown`으로 남긴다.

- 이름 있는 Job을 만들지 못했거나 watcher를 시작하지 못하면 새 종료 증거가 생기지 않는다. 실행기가 watcher의 Job·실행기 핸들 확보 전에 죽어 열기나 신원 확인에 실패한 경우도 기록하지 않는다.
- watcher가 죽으면 그 핸들이 닫힌다. 실행기가 살아 있으면 실행기의 Job 핸들이 남고, 마지막 Job 핸들까지 닫히면 `KILL_ON_JOB_CLOSE`가 종료를 요청한다. 다만 완료를 확인해 쓸 watcher가 없으므로 종료 확인이 없는 예약은 계속 `unknown`이다.
- 프로세스나 진행 중인 I/O가 멈춰 `ActiveProcesses`가 0이 되지 않거나 보유한 핸들이 신호되지 않으면 watcher는 기한 없이 기다리며 확인을 남기지 않는다.
- 핸들 대기·Job 종료·회계 정보 조회·DB 갱신이 실패하거나 예약 행이 바뀌면 확인을 지어내지 않는다. 다른 예약을 갱신하거나 자동으로 자원을 풀지 않는다.

예약을 푸는 것은 여전히 사람이 한다. 앱의 "예약 복구" 버튼이나 `o resources` 복구 명령을 쓴다. O가 자동으로 풀지 않는다. 카드는 실제로 시작했으므로 그날의 시작 수(`starts`)는 돌려주지 않는다.

### 4. 그대로 두는 것

`LIMITS`, 자원 충돌 규칙, 기존 복구 경우(시작하지 않은 예약, 종료가 확인된 예약), 후속 실행 문구, `events.jsonl` 형식.

## 이유

- **증거 기반**: "살아 있지 않아 보임"은 증거가 아니다. Windows에서 열 권한이 없는 프로세스를 죽은 것으로 읽으면, 아직 파일을 쓰는 프로세스가 있는데 다른 카드가 같은 파일을 받는다. 실행 닫기는 pid·생성 시각을 함께 검사하는 `definitely_exited`를 쓰고, 시작한 예약의 복구에는 실행기 종료와 카드 전체의 `exit_confirmed`가 필요하다. 실행기나 작업 프로세스 하나의 종료는 자손 종료를 증명하지 않는다. watcher가 열린 핸들을 신뢰하기 전에는 전달받은 생성 시각과 예약 행이 일치해야 한다.
- **실행 닫기는 자동, 예약 풀기는 사람**: 실행을 닫는 것은 기록만 바꾸므로 잘못 닫혀도 파일이 덮이지 않는다. 예약을 푸는 것은 다른 카드가 같은 파일을 쓰게 하므로, 증거가 갖춰져도 사람이 작업 폴더를 확인한 뒤 누르게 둔다.
- **부모 pid 추적을 버린 이유**: 처음 안(2145dcb)은 ToolHelp 스냅샷의 부모 pid로 작업 프로세스의 자손을 따라갔다. 두 가지가 틀렸다.
  - 중간 프로세스가 끝나 스냅샷에서 사라지면 그 아래 손자는 작업 프로세스와 이어지지 않는다. 그러면 카드의 프로세스가 아직 도는데 "자손 없음"이 된다.
  - 스냅샷이 오류로 중간에 끝나거나 프로세스를 열지 못해도 "없음"으로 읽힐 수 있었다.
  - 그래서 그 검사(`descendants_alive`)를 지웠다.
- **Job 이름 부재를 증거로 쓰지 않는 이유**: Microsoft의 [Life Cycle of an Object](https://learn.microsoft.com/en-us/windows-hardware/drivers/kernel/life-cycle-of-an-object)는 임시 객체의 핸들 수가 0이 되면 이름이 사라지지만 참조가 남은 객체는 계속 존재할 수 있다고 설명한다. [Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)는 `KILL_ON_JOB_CLOSE`의 마지막 핸들 닫기가 소속 프로세스를 종료한다고 설명한다. 그러나 [TerminateJobObject](https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-terminatejobobject)는 각 소속 프로세스에 `TerminateProcess`를 호출하는 것과 같고, [TerminateProcess](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-terminateprocess)는 외부 호출 시 비동기이며 진행 중인 I/O가 완료되거나 취소될 때까지 프로세스가 끝나지 않는다. 따라서 이름 부재를 실제 종료 완료로 취급할 수 없다는 것이 이 결정의 판단이다.
- **Job이 비었다는 확인**: watcher는 검증한 Job 핸들을 유지한 채 명시적으로 종료를 요청하고 `ActiveProcesses == 0`을 기다린다. Microsoft의 [JOBOBJECT_BASIC_ACCOUNTING_INFORMATION](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_accounting_information)은 이 값이 현재 Job에 속한 프로세스 수이며 종료한 프로세스가 나가고 참조가 해제될 때 줄어든다고 설명한다. 이 확인 뒤에도 정확히 일치하는 예약 행에만 종료 증거를 기록한다.
- **Windows만**: 이 watcher의 종료 증거는 Windows Job Object에 한정된다. Linux·macOS에서 실행기가 죽고 종료 확인이 남지 않은 예약은 계속 `unknown`이다. 정상 실행기가 남긴 기존 종료 확인과 시작하지 않은 예약의 복구는 그대로다.
- **예전 예약은 그대로 `unknown`**: 실행기가 죽고 `exit_confirmed`가 남지 않은 기존 예약에는 새 증거를 소급하지 않는다. `job_name`이 없는 예약과 watcher 도입 전에 시작한 예약 모두 해당한다. 배경의 `20261109-123716-tent` `gui` 예약도 그렇다. 이름 부재나 경과 시간으로 확인을 대신하지 않으며, 사람이 작업 폴더와 프로세스를 직접 확인해야 한다.

## 결과

- `20261109-123716-tent` 같은 실행은 다음에 `o status`·`o runs`·앱이 읽을 때 `failed`로 닫힌다. 닫히지 못한 카드에는 "중단됨" 사유가 붙고, 그 카드를 다시 보낼 수 있다(후속 대상은 `card.end`가 있어야 한다). 그 후속 지시는 끊긴 세션을 이어 가지 않고 같은 작업 폴더에서 새 세션으로 시작한다.
- Windows에서 watcher가 실행기 종료 후 Job을 종료하고 활성 프로세스 수 0과 보유한 프로세스 핸들의 종료 신호를 확인해 해당 예약에 `exit_confirmed`를 남기면 예약이 "복구 가능"으로 바뀐다. 사람이 "예약 복구"를 누르면 풀린다. 이름만 사라진 예약, watcher가 확인하지 못한 예약, 종료 확인이 없는 예전 예약(`tent`의 `gui` 포함)과 Linux·macOS의 예약은 `unknown`으로 남는다.
- 총괄 개요(`lead/overall.rs`)는 이벤트만 읽고 이 정리를 부르지 않는다. 같은 실행을 `o runs`·앱·`o status`가 읽으면 기록이 닫히므로 다음 개요부터 맞는다.
- 검증 기준(실행 결과는 구현 검증에서 별도 확인):
  - `crates/o-core/tests/dead_runner.rs`:
    - 끝난 실행기를 가리키는 실행이 한 번만 닫히는지 본다.
    - 살아 있는 실행기·`runner.json`이 없는 실행·시작 시각을 모르는 실행·살아 있는 카드 표식이 있는 실행·계획을 읽지 못하는 실행은 그대로인지 본다.
    - 오래된 이벤트를 든 읽기가 다른 읽기가 닫은 상태를 돌려받는지 본다.
    - 중단된 카드의 후속 대상이 새 세션인지 본다.
    - `job_name` 없는 예약이 `unknown`인지 본다.
  - `run::job_watch`의 Windows 단위 검사:
    - 제어 가능한 별도 실행기와 실제 작업 프로세스·손자를 생산 코드의 이름 있는 Job으로 띄우고, `reserve_slot`·`link_slot`으로 만든 예약을 쓴다. 실행기 종료 전에는 확인이 없어야 한다. 실행기를 종료하면 watcher가 Job을 종료하고, 작업 프로세스와 손자의 핸들이 모두 종료 신호를 낸 뒤에만 `exit_confirmed`가 기록되는 순서를 확인한다. 이후 `recoverable` 표시와 실제 복구까지 본다.
    - Job 이름이 사라져도 `exit_confirmed`가 없으면 `unknown`인지 본다.
    - 작업 프로세스 pid·생성 시각, Job 이름 또는 예약 식별자가 다른 행은 갱신되지 않아야 한다.
    - watcher가 Job 핸들을 들고 있어도 카드의 정상 종료·중지·시간 초과·오류 경로에서 프로세스가 종료되어야 한다.
    - 시간 대기로 수명을 맞추지 않고 핸들·이벤트·입력 파이프·파일로 제어한다.
  - `crates/o-core/tests/runner.rs`: 중단된 카드의 후속 실행이 `--resume` 없이 시작하고, CLI가 알린 새 세션을 다음 후속 지시가 이어 가는지 본다.
  - 후속 잠금 검사: 예전 fresh 키와 같은 실제 세션 ID의 재개와 fresh 후속 실행이 서로 막지 않아야 한다. 서로 다른 `(run, card)` 조합은 다른 잠금을 얻고, 같은 조합의 중복 실행은 막혀야 한다.
  - `win_launch` 단위 테스트: 이미 있는 Job 이름을 받아 쓰지 않는지 본다.
