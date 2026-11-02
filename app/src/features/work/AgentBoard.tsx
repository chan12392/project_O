import { GitPullRequest } from "lucide-react";

import { prOf, tokensOf } from "../../../../src/core/attention.ts";
import { blockedReason, failedBeforeCards, type CardView, type RunView } from "../../../../src/core/snapshot.ts";
import { PROVIDER_NAME, STATE_WORD, clip, compact, modelLine, timeOf } from "../../lib/format.ts";
import { ExternalAnchor } from "../../ui/ExternalAnchor.tsx";
import { lookOf } from "../office/characters.ts";
import { Avatar } from "../office/Avatar.tsx";
import { KanbanBoard, KanbanCard, KanbanCards, KanbanHeader, KanbanProvider, type KanbanColumn } from "./Kanban.tsx";
import { ReviewButton, isReviewCard } from "./ReviewRequest.tsx";

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
  const blockedRuns = runs.filter(failedBeforeCards);
  return (
    <KanbanProvider columns={COLUMNS}>
      {(column) => {
        const inColumn = items.filter((i) => columnOf(i.card) === column.id);
        const failedRuns = column.id === "blocked" ? blockedRuns : [];
        const count = inColumn.length + failedRuns.length;
        return (
          <KanbanBoard key={column.id} id={column.id}>
            <KanbanHeader id={column.id}>
              <span className={`size-2 rounded-full ${column.tone}`} aria-hidden="true" />
              {column.name}
              <span className="ml-auto rounded-full bg-muted px-1.5 text-3xs font-normal text-muted-foreground">{count}</span>
            </KanbanHeader>
            {/* Stacked, an empty lane is just its header; the PR lane keeps a line saying why it is empty. */}
            <KanbanCards empty={count === 0 && column.id !== "review"}>
              {count === 0 && <p className="px-1 py-1 text-2xs text-muted-foreground">{column.id === "review" ? "에이전트가 보고한 PR이 없습니다" : "없음"}</p>}
              {failedRuns.map((run) => (
                <div key={run.runId} className="rounded-lg border p-3 text-xs">
                  <p className="font-medium">{run.title} · 카드 시작 전 실패</p>
                  <p className="mt-1 whitespace-pre-wrap break-words text-muted-foreground">{blockedReason(run)}</p>
                </div>
              ))}
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

function BoardCard({ card, run, onOpen }: { card: CardView; run: RunView; onOpen: () => void }) {
  const pr = prOf(card);
  const tools = card.log.filter((l) => l.kind === "tool").length;
  const tokens = tokensOf(card);
  const reviewer = isReviewCard(card);
  return (
    <div className="flex flex-col gap-1">
      <KanbanCard action="카드 기록 열기" onOpen={onOpen}>
        <span className="flex items-center gap-2">
          <Avatar speaker={card.provider} look={lookOf(run, card.id)} size="sm" />
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
      </KanbanCard>
      {/* Outside the card's button: the PR link and the review request are their own controls. */}
      {pr && (
        <div className="flex flex-col gap-1 px-1 pb-1">
          <span className="flex min-w-0 items-center gap-2 text-2xs">
            <ExternalAnchor href={pr} className="flex min-w-0 items-center gap-1 truncate text-callout-note hover:underline focus-ring">
              <GitPullRequest className="size-3 shrink-0" aria-hidden="true" />
              {pr.replace("https://github.com/", "")}
            </ExternalAnchor>
            <span className="shrink-0 text-muted-foreground">{reviewer ? "에이전트 검토 완료 · 사람 판단" : "사람 검토 필요"}</span>
          </span>
          {!reviewer && <ReviewButton project={run.project} pr={pr} />}
        </div>
      )}
    </div>
  );
}

/** A flowing bar while work is in flight, a full one when it lands. */
function Progress({ state }: { state: CardView["state"] }) {
  const running = state === "running";
  const fill = state === "done" ? "w-full bg-emerald-500" : state === "failed" ? "w-full bg-red-500/70" : state === "stopped" ? "w-full bg-muted-foreground/50" : "w-0";
  return (
    <span className="relative block h-1 overflow-hidden rounded-full bg-muted" aria-hidden="true">
      {running ? <span className="progress-flow absolute inset-y-0 w-2/5 rounded-full bg-agent-working" /> : <span className={`block h-full rounded-full ${fill}`} />}
    </span>
  );
}
