# project_O

여러 AI 코딩 에이전트와 구독 계정을 한 데스크톱 앱에서 실행하고(작업대), 돌고 있는 세션을 한 화면에서 관측하는(관제탑) 개인용 로컬 앱. 비유하면 Orca의 채팅 UI 버전.

> 상태: O CLI(작업카드 분배)와 정책 자동 차단까지 동작. 채팅 UI·캐릭터 화면은 다음 단계.

- 작업 규칙·브랜치 흐름·정책 차단: [AGENTS.md](AGENTS.md)
- 설계 초안: [docs/DESIGN.md](docs/DESIGN.md)
- 결정 기록: [docs/decisions/](docs/decisions/)

## 시작하기

Node.js 22.18 이상. 의존성은 타입 검사용뿐이고 실행은 Node만으로 된다.

```sh
npm install        # git 훅(.githooks) 연결 포함
npm test           # 정책·훅·스트림 해석·실행기 테스트
npm run check      # TypeScript
npm run guard      # 전체 파일 정책 검사
npm link           # 선택: 어디서나 `o` 명령 사용
```

## O CLI

총괄 에이전트(예: Claude Code 세션)가 요청을 작업카드로 나눠 넘기면, O CLI가 정책을 검사한 뒤 각 공식 CLI를 백그라운드에서 병렬로 실행한다.

```
사용자 ─▶ 총괄 Claude ─(O 스킬)─▶ o dispatch plan.json
                                   │ 정책 검사 (위반 시 전체 차단, 종료 코드 2)
                                   ├─▶ codex exec --json      (코딩)
                                   ├─▶ agy -p --output-format stream-json  (디자인)
                                   └─▶ claude -p --output-format stream-json (설계)
                                         ▼
                      %LOCALAPPDATA%/project_O/runs/<runId>/events.jsonl ─▶ 채팅 UI·캐릭터 화면
```

| 명령 | 하는 일 |
|---|---|
| `o check plan.json` | 정책 검사만 |
| `o dispatch plan.json` | 검사 후 백그라운드 실행, runId 출력 |
| `o status [runId]` | 카드별 상태 |
| `o events <runId> [--follow]` | 정규화 이벤트(JSONL) |
| `o stop <runId>` | 중지 |

계획 형식과 총괄 에이전트 사용법은 [skills/o/SKILL.md](skills/o/SKILL.md). Claude Code에서 쓰려면 이 폴더를 `~/.claude/skills/o`로 복사하거나 링크한다.
