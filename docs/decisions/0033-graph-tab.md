# ADR-0033: Graph 탭 — 프로젝트 코드 그래프를 3D로 보고, grafly 기능을 `o graph`로 가져온다

- 상태: 채택
- 날짜: 2026-11-08
- 별도 승인 대기 없음 (ADR-0012)
- 관련: [ADR-0019](0019-navigation-images-schedules.md)(왼쪽은 전체), [ADR-0032](0032-overall-lead.md)(전체 총괄), [ADR-0004](0004-daily-usage-budget.md)(하루 예산)

## 배경

- 대표 요청: 왼쪽 위에 Graph 카테고리를 두고, 전체 프로젝트 그래프에서 프로젝트별 그래프로 뻗어 가는 3D 그래프를 본다. 작업 중인 곳과 그 연결 경로가 빛나고, 효과는 켜고 끌 수 있다.
- grafly(별도 도구, 저장소는 아직 공개되지 않았다)는 코드를 파싱해 `.grafly/.graph/wiring.json`(노드·간선)을 만들고, LLM 없이 호출 관계(`callers`), 변경 영향(`blast`), 파일 API(`skeleton`), 저장소 개요(`map`)를 답한다. `--deep`의 개념 요약·영역 이름 짓기만 LLM을 쓴다.
- grafly의 에이전트 Deep(`deepidx`)은 로그인된 Claude·Codex CLI를 쓰지만 모델이 코드에 고정돼 있다(Claude `haiku`, Codex `luna` 계열). 대표는 기본 모델을 Codex `gpt-6.1-terra`, Claude `sonnet`(Sonnet 5.5)으로 두고 바꿀 수 있기를 원한다.

## 결정

### 무엇을 코드로, 무엇을 LLM으로

| 일 | 맡는 곳 | LLM |
|---|---|---|
| 인덱스 만들기 | `grafly build`(파서) — O가 하위 프로세스로 호출 | 없음 (`--deep`은 절대 넘기지 않음) |
| 그래프 신선도 | `grafly check` | 없음 |
| 위치 찾기 | `grafly ask --json` | 없음 |
| 호출 관계, 변경 영향, 파일 API, 개요 | **O가 `wiring.json`을 직접 읽어 Rust로 계산** (`o-core::graph`) | 없음 |
| 3D 화면용 그래프, 빛나는 노드·경로 | O(`git status` + 영향 범위 계산) | 없음 |
| 영역 이름 짓기, 개념 요약 (Deep) | O가 사람이 고른 공식 CLI·모델을 읽기 전용·도구 없이 한 번 호출 | **사용** (사용자 구독 차감) |

grafly가 설치되지 않았거나 인덱스가 없으면 화면은 그 사실과 "인덱스 만들기"를 보여 준다. O는 grafly를 내려받거나 설치하지 않는다. Graph 탭을 쓰려면 grafly를 따로 설치해 PATH에 둬야 한다. grafly가 없으면 화면은 "grafly가 설치되어 있지 않습니다"를 보여 주고, 다른 기능은 그대로 쓸 수 있다.

### CLI (`o graph`)

```
o graph view [--project <폴더>] --json            3D 화면용 그래프 (없으면 전체)
o graph build --project <폴더>                    grafly build ($0)
o graph check --project <폴더>                    grafly check
o graph ask <질의> --project <폴더> --json         grafly ask --json
o graph callers <심볼> --project <폴더> [--direction in|out] [--depth N|all] --json
o graph blast --project <폴더> [--base <ref>] [--depth N] --json
o graph skeleton <파일> --project <폴더> --json
o graph map --project <폴더> --json
o graph deep --project <폴더> [--provider codex|claude] [--model M] [--profile project-o|own] --json
o graph settings [--effects on|off] [--particles on|off] [--deep-provider codex|claude]
                 [--codex-model M] [--claude-model M] [--warn-deep on|off]
                 [--max-nodes N] [--depth N] --json
```

### 화면용 데이터 (`o graph view --json`, camelCase)

```ts
interface GraphView {
  scope: "overall" | "project";
  project?: string;                 // scope가 project일 때 그 폴더
  grafly: { installed: boolean; version?: string };
  indexed: boolean;                 // project: wiring.json 있음, overall: 하나라도 있음
  nodes: GraphNode[];
  links: GraphLink[];
  truncated: boolean;               // maxNodes로 잘렸는지
  deep?: { at: string; provider: string; model: string };
}
interface GraphNode {
  id: string;                       // "o:root" | "p:<경로>" | "a:<경로>|<폴더>" | "d:<폴더>" | "f:<파일>"
  label: string;
  kind: "root" | "project" | "area" | "file";
  project?: string;
  path?: string;                    // 파일·폴더의 프로젝트 기준 상대 경로
  size: number;                     // 파일 수 또는 심볼 수 (1 이상)
  glow?: "active" | "path";         // active: 지금 바뀐 곳, path: 그 영향 경로
  summary?: string;                 // Deep 요약
  indexed?: boolean;                // project 노드: 인덱스 있음
}
interface GraphLink {
  source: string; target: string;
  kind: "contains" | "calls" | "imports" | "references";
  weight: number;                   // 같은 파일 쌍의 간선 수
  glow?: boolean;                   // 빛나는 경로 위의 간선
}
```

- **전체(overall):** `o:root` → 목록의 프로젝트(`p:`) → 프로젝트마다 큰 폴더 영역(`a:`, 최대 12개). 프로젝트·영역은 안에 바뀐 파일이 있으면 `glow: "active"`, 그 위로 이어지는 `contains` 간선은 `glow: true`.
- **프로젝트(project):** 프로젝트 노드 → 폴더 영역(`d:`, 깊이 2까지) → 파일(`f:`). 파일 사이 간선은 `wiring.json`의 심볼 간선(`calls`·`imports`·`references`)을 파일 단위로 묶은 것.
- **빛나는 곳:** 프로젝트 폴더의 `git status`(추적 안 된 파일 포함)에서 바뀐 파일이 `active`. 그 파일의 심볼에서 들어오는 간선을 설정 깊이(기본 2)만큼 거슬러 올라간 파일이 `path`, 그 경로의 간선이 `glow: true`. 정적 그래프라서 문자열로 이어진 관계(Tauri `invoke`, 이벤트 형식, 프로세스 인자)는 빛나지 않는다.
- **노드 상한:** 설정 `maxNodes`(기본 1500, 100~5000). 빛나는 노드는 항상 남기고 나머지는 연결 수가 많은 파일부터.

### Deep

- 그래프 화면의 "Deep" 버튼. 처음 누를 때 "LLM 사용량이 차감됩니다. 상위 모델일수록 차감량이 늘어납니다"와 "다음부터 안 보기"를 보여 준다(설정 `warnDeep`).
- 한 번 누르면 공식 CLI를 **한 번** 부른다: 영역 목록(폴더, 파일 수, 대표 파일, 연결 많은 심볼과 grafly 요약)을 주고 `{"areas":[{"id","name","summary"}]}` JSON을 받는다. 영역은 최대 40개, 이름 24자·요약 200자 이내, 목록에 없는 id는 버린다.
- 실행: Codex `exec --sandbox read-only --skip-git-repo-check --ephemeral`(셸·웹·플러그인 끔), Claude `-p --tools "" --strict-mcp-config` 빈 MCP, 프로젝트 훅 끔. 임시 폴더에서 돌고, 창은 숨기고, 10분 제한. 자식 프로세스의 인증 환경은 ADR-0035를 따른다. `--profile project-o`(기본)면 깨끗한 프로필.
- 하루 예산이 켜져 있고 그 서비스를 오늘 다 썼거나, 한도로 쉬는 중이면 거부한다.
- 결과는 O 상태 폴더 `graph/<프로젝트 키>/deep.json`에 저장하고 `view`가 영역 이름과 요약으로 쓴다. 프로젝트 폴더에는 쓰지 않는다.

### 설정 (`<O 상태 폴더>/graph/settings.json`)

`effects`(빛남, 기본 켜짐), `particles`(경로 위를 흐르는 점, 기본 켜짐), `deepProvider`(기본 `codex`), `codexModel`(기본 `gpt-6.1-terra`), `claudeModel`(기본 `sonnet`), `warnDeep`(기본 켜짐), `maxNodes`(1500), `depth`(2, 1~4). 앱 설정에 "Graph" 섹션을 두고 같은 값을 바꾼다. "Wiki" 섹션은 준비 중으로 둔다.

## 영향과 한계

- 화면은 Tauri 명령(`graph_view`, `graph_build`, `graph_deep`, `graph_settings`)만 부르고, 개발 서버 `/api/graph/*`는 같은 `o graph` 명령을 쓴다.
- 3D 표시는 `3d-force-graph`(MIT, three.js)를 앱에 번들한다. 외부 CDN을 쓰지 않는다.
- 카드의 분리 워크트리에서 바뀐 파일은 프로젝트 폴더의 `git status`에 없으므로 빛나지 않는다. 실행 중 카드의 작업 폴더를 합치는 것은 다음 단계다.
- Deep은 grafly의 `--deep` 결과(`.grafly/*.md`)를 바꾸지 않는다. O의 Deep 결과는 O 화면에서만 쓴다.

## 개정 1 (2026-11-08): 나눠 받기, 펼쳐지는 등장, 색

- **나눠 받기:** 전체 그래프는 프로젝트 목록으로 루트와 프로젝트 노드를 바로 그리고, 프로젝트마다 `o graph part --project <폴더> --json`(Tauri `graph_overall_part`, 개발 서버 `/api/graph/part`)을 최대 4개씩 받아 합친다. 응답은 그 프로젝트 노드와 영역, `contains` 간선만 담는다(루트 없음). `o graph view`(전체)는 예전 `o`나 데모를 위해 남긴다. 프로젝트 목록이 나중에 도착하면 다시 불러오되 이미 펼쳐진 노드는 남긴다.
- **속도:** 정책 컨텍스트는 한 번만 만들고 프로젝트는 최대 4개씩 병렬로 처리한다. 인덱스 없는 프로젝트는 git을 부르지 않고, 전체 그래프는 영역 단위로만 빛난다. 프로젝트별 결과는 인덱스 파일·git index·HEAD로 캐시하고(64개), `grafly --version`은 프로세스마다 한 번만 실행한다. `git status`는 바뀐 파일이 바로 빛나도록 매번 다시 한다.
- **펼쳐지는 등장:** 루트 → 프로젝트 → 영역 → 파일 순으로, 새 노드는 부모 자리에서 태어나 바깥으로 퍼진다. 전체 약 1.8초 안에 묶음으로 펼치고, 끝난 뒤 빛남과 흐름이 켜진다. 같은 범위의 새로고침은 다시 펼치지 않는다. 효과를 끄거나 동작 줄이기가 켜져 있으면 한 번에 뜬다. 범위를 바꾸면 배치가 멈춘 뒤 카메라가 그래프 전체에 맞춘다.
- **색:** 설정 `colorBy`(`language` 기본, `area`). 언어별은 파일 확장자로, 영역별은 최상위 폴더마다 색을 준다. 영역 노드는 두 기준 모두 영역색을 쓴다. 지금 바뀐 파일만 빛남 색으로 칠하고, 영향 경로의 노드는 제 색을 유지한 채 후광과 흐름으로 표시한다. 밝은·어두운 테마의 색을 따로 둔다. 테마 색(`oklch()` 등)은 캔버스에 칠해 읽은 sRGB 값으로 WebGL에 넘긴다.
- **기본 모델:** 모델 설정의 빈 값은 CLI 자체의 기본 모델이다(CLI 인자로는 `default`). Deep은 `--model`을 넘기지 않고 결과의 모델을 `default`로 남긴다. Codex Deep은 격리를 위해 사용자 설정을 읽지 않으므로 Codex CLI에 내장된 기본 모델을 쓴다.
- **스타일:** 앱의 CSP(`style-src 'self'`)가 3D 라이브러리들이 실행 중에 넣는 `<style>`을 막으므로, 같은 규칙을 `app/src/styles/graph.css`로 번들한다. 라이브러리의 영어 마우스 안내는 끈다.

## 개정 2 (2026-11-10): Dreaming 화면의 탭, 묶어서 그리기

- Graph는 왼쪽 위의 독립 항목이 아니라 Dreaming 화면의 첫 탭이다. 설정의 "Wiki" 섹션은 없앤다([ADR-0041](0041-dreaming.md)).
- 3D 표시는 배치를 Web Worker에서 끝낸 뒤 노드·링크·빛을 묶어 그린다. `3d-force-graph`는 캔버스·카메라·마우스 조작만 맡는다. 개정 1의 "펼쳐지는 등장"과 색 규칙은 그대로이고, 효과가 켜져 있으면 모든 노드가 옅은 빛을 두른다(ADR-0041).
