import { GitPullRequest } from "lucide-react";

import { prOf } from "../../../src/core/attention.ts";
import type { CardView, RunView } from "../../../src/core/snapshot.ts";
import { PROVIDER_NAME, STATE_WORD, clip, modelLine, timeOf } from "../format.ts";
import { lookFor } from "../characters.ts";
import { Avatar } from "./Avatar.tsx";
import { KanbanBoard, KanbanCard, KanbanCards, KanbanHeader, KanbanProvider, type KanbanColumn } from "./Kanban.tsx";

type ColumnId = "waiting" | "running" | "review" | "done" | "blocked";
const COLUMNS: (KanbanColumn & { id: ColumnId; tone: string })[] = [
  { id: "waiting", name: "대기", tone: "bg-neutral-500" },
  { id: "running", name: "작업 중", tone: "bg-agent-working" },
  { id: "review", name: "PR·검토", tone: "bg-sky-500" },
  { id: "done", name: "완료", tone: "bg-emerald-500" },
  { id: "blocked", name: "막힘·중지", tone: "bg-red-500" },
];

function columnOf(card: CardView): ColumnId {
  if (card.state === "running") return "running";
  if (card.state === "failed" || card.state === "stopped") return "blocked";
  if (card.state === "done") return prOf(card) ? "review" : "done";
  return "waiting";
}

interface Item {
  card: CardView;
  run: RunView;
}

/** The project's work cards on a board: who does what, which PR, how far along. */
export function AgentBoard({ runs, onOpen }: { runs: RunView[]; onOpen: (target: { run: string; card: string }) => void }) {
  const items: Item[] = runs.flatMap((run) => run.cards.map((card) => ({ card, run })));
  return (
    <KanbanProvider columns={COLUMNS}>
      {(column) => {
        const inColumn = items.filter((i) => columnOf(i.card) === column.id);
        return (
          <KanbanBoard key={column.id} id={column.id}>
            <KanbanHeader id={column.id}>
              <span className={`size-2 rounded-full ${column.tone}`} aria-hidden="true" />
              {column.name}
              <span className="ml-auto rounded-full bg-muted px-1.5 text-3xs font-normal text-muted-foreground">{inColumn.length}</span>
            </KanbanHeader>
            <KanbanCards>
              {inColumn.length === 0 && <p className={`px-1 py-1 text-2xs text-muted-foreground ${column.id === "review" ? "" : "@max-4xl:hidden"}`}>{column.id === "review" ? "에이전트가 보고한 PR이 없습니다" : "없음"}</p>}
              {inColumn.map(({ card, run }) => (
                <BoardCard key={`${run.runId}/${card.id}`} card={card} run={run} onOpen={() => onOpen({ run: run.runId, card: card.id })} />
              ))}
            </KanbanCards>
          </KanbanBoard>
        );
      }}
    </KanbanProvider>
  );
}

const compact = new Intl.NumberFormat("ko-KR", { notation: "compact", maximumFractionDigits: 1 });

function BoardCard({ card, run, onOpen }: { card: CardView; run: RunView; onOpen: () => void }) {
  const pr = prOf(card);
  const tools = card.log.filter((l) => l.kind === "tool").length;
  const tokens = card.usage.input + card.usage.cached + card.usage.output;
  return (
    <KanbanCard action="카드 기록 열기" onOpen={onOpen}>
      <span className="flex items-center gap-2">
        <Avatar speaker={card.provider} look={lookFor(card.id, run.cards.map((c) => c.id))} size="sm" />
        <span className="min-w-0">
          <span className="block truncate text-sm font-medium">{card.label}</span>
          <span className="block truncate font-mono text-2xs text-muted-foreground">
            {PROVIDER_NAME[card.provider]} {modelLine(card.model, card.effort)}
          </span>
        </span>
      </span>
      <span className="line-clamp-2 text-xs text-muted-foreground">{clip(card.activity || "아직 기록 없음", 90)}</span>
      <Progress state={card.state} />
      <span className="flex flex-wrap items-center gap-x-2 gap-y-1 text-2xs text-muted-foreground">
        <span className="max-w-full truncate rounded-sm bg-muted px-1 text-foreground">{run.title}</span>
        <span>{STATE_WORD[card.state]}</span>
        {card.startedAt && (
          <span>
            {timeOf(card.startedAt)}
            {card.endedAt && `–${timeOf(card.endedAt)}`}
          </span>
        )}
        {tools > 0 && <span>도구 {tools}회</span>}
        {tokens > 0 && <span>{compact.format(tokens)} 토큰</span>}
      </span>
      {pr && (
        <span className="flex items-center gap-1 truncate text-2xs text-sky-400">
          <GitPullRequest className="size-3 shrink-0" aria-hidden="true" />
          {pr.replace("https://github.com/", "")}
        </span>
      )}
    </KanbanCard>
  );
}

/** Pattern from beakho-design lucian-083: a flowing bar while work is in flight, a full one when it lands. */
function Progress({ state }: { state: CardView["state"] }) {
  const running = state === "running";
  const fill = state === "done" ? "w-full bg-emerald-500" : state === "failed" ? "w-full bg-red-500/70" : state === "stopped" ? "w-full bg-muted-foreground/50" : "w-0";
  return (
    <span className="relative block h-1 overflow-hidden rounded-full bg-muted" aria-hidden="true">
      {running ? <span className="progress-flow absolute inset-y-0 w-2/5 rounded-full bg-agent-working" /> : <span className={`block h-full rounded-full ${fill}`} />}
    </span>
  );
}
