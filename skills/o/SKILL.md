---
name: o
description: Split a multi-part request into work cards and hand them to agent CLIs (Codex, Antigravity, Claude) through O CLI, let the cards talk to each other and to you on the run's team channel, then watch and summarize the results. Use when the user asks to run parts of a project in parallel on different models or CLIs, or names providers/models per task.
---

# O — 작업카드 분배

총괄 에이전트는 계획만 세운다. 실행·권한·한도 판단은 O CLI가 한다.

## 1. 계획 만들기

사용자 요청을 서로 독립적인 카드 2~5개로 나눈다. 카드끼리 같은 파일을 동시에 고치지 않게 나눈다.

```json
{
  "title": "랜딩 페이지 개편",
  "project": "C:/Users/me/projects/landing",
  "cards": [
    { "id": "ui", "provider": "codex", "model": "gpt-6.1-sol", "effort": "xhigh",
      "task": "무엇을, 어느 파일에서, 완료 기준은 무엇인지" },
    { "id": "copy", "provider": "agy", "model": "gemini-3.8-flash-high", "role": "문구",
      "task": "..." }
  ]
}
```

- `provider`: `claude` | `codex` | `agy`(Antigravity CLI). 모델은 **사용자가 정한 것을 그대로** `model`에 넣고, 정하지 않았으면 비워 둔다(각 CLI 기본 모델). 작업 종류로 모델을 고르지 않는다.
- `role`은 선택 사항인 사람용 이름표다("코딩", "검토" 등). 모델이나 권한과 아무 관계가 없다. 없으면 화면에 카드 id로 보인다.
- 허용 필드는 `id, provider, task, role, model, effort`뿐이다. 실행 파일·인자·환경변수·계정은 넣을 수 없다.
- `task`는 짧고 자족적으로 쓴다: 목표, 범위(파일·폴더), 완료 기준. 다른 카드와 주고받아야 할 것이 있으면 task에 적는다("끝나면 ui 카드에게 바꾼 파일을 알려 줄 것").

## 2. 검사 후 전달

```sh
o check plan.json      # 정책 검사만
o dispatch plan.json   # 통과하면 백그라운드 병렬 실행, runId 출력
```

명령은 셸 도구로 **한 줄에 하나씩**, 계획 파일은 **절대 경로**로 실행한다(`cd ... &&`, `; echo $?` 같은 덧붙임 없이). 덧붙인 명령은 따로 승인을 요구해 흐름이 멈춘다. 성공·차단은 출력으로 판단한다: 통과는 `정책 통과`, 전달 성공은 runId JSON, 차단은 `차단됨`으로 시작한다.

종료 코드 2는 **정책 차단**이다. 출력된 사유를 사용자에게 그대로 보고하고 멈춘다. 같은 서비스의 다른 계정으로 바꾸거나, 필드를 바꿔 끼워 검사를 피하지 않는다.

## 3. 팀 대화

카드들은 같은 실행 안에서 서로, 그리고 총괄(`lead`)에게 메시지를 보낼 수 있다. 각 카드는 작업카드에서 방법을 안내받는다. 총괄은 이렇게 쓴다:

```sh
o msg read --run <runId> --as lead          # 나에게 온 새 메시지
o msg send --run <runId> --as lead --to <카드 id|all> "내용"
o msg read --run <runId> --all              # 전체 대화
```

메시지도 작업과 같은 정책 검사를 받는다. 카드의 질문에는 짧게 답하고, 결정이 필요한 것은 사용자에게 묻는다.

## 4. 지켜보고 정리하기

```sh
o status <runId>             # 카드별 상태 한 줄씩
o events <runId> --follow    # 이벤트 스트림 (끝나면 종료)
o stop <runId>               # 중지
```

모든 카드가 끝나면 카드별 결과(`card.end`의 `result`)와 팀 대화에서 오간 중요한 내용을 모아 사용자에게 요약한다: 카드마다 한 일, 실패·차단된 카드와 이유, 다음에 할 일.
