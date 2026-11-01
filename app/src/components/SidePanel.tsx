import { Building2, MessagesSquare, SquareTerminal } from "lucide-react";

import type { RunView } from "../../../src/core/snapshot.ts";
import { PROVIDER_SHORT, names, timeOf } from "../format.ts";
import type { PanelView } from "../useRun.ts";
import { AgentStateDot } from "./AgentStateDot.tsx";
import { CardTranscript } from "./CardTranscript.tsx";
import { Office } from "./Office.tsx";

const VIEWS: { id: PanelView; label: string; icon: typeof Building2 }[] = [
  { id: "work", label: "에이전트 작업", icon: SquareTerminal },
  { id: "office", label: "사무실", icon: Building2 },
  { id: "team", label: "팀 대화", icon: MessagesSquare },
];

interface Props {
  run?: RunView;
  view: PanelView;
  card?: string;
  onView: (view: PanelView) => void;
  onCard: (id: string) => void;
}

/** The right half of the split: pick what to watch beside the lead chat. */
export function SidePanel({ run, view, card, onView, onCard }: Props) {
  const current = run?.cards.find((c) => c.id === card) ?? run?.cards[0];
  return (
    <section className="flex min-h-0 min-w-0 flex-col border-l" aria-label="작업 보기">
      <div role="tablist" aria-label="보기 고르기" className="flex h-9 shrink-0 items-stretch border-b bg-tab-bar">
        {VIEWS.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            type="button"
            role="tab"
            aria-selected={view === id}
            onClick={() => onView(id)}
            className="relative flex items-center gap-1.5 border-r px-3 text-xs text-tab-inactive-foreground hover:bg-accent/40 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none focus-visible:ring-inset aria-selected:bg-tab-active aria-selected:text-tab-active-foreground"
          >
            <Icon className="size-3.5" aria-hidden="true" />
            {label}
            {id === "team" && run && run.messages.length > 0 && (
              <span className="rounded-full bg-muted px-1.5 text-3xs text-muted-foreground">{run.messages.length}</span>
            )}
            {view === id && <span className="absolute inset-x-0 bottom-0 h-px bg-tab-active-indicator" aria-hidden="true" />}
          </button>
        ))}
      </div>

      {!run && <p className="p-5 text-sm text-muted-foreground">작업을 고르면 여기서 볼 수 있습니다.</p>}

      {run && view === "work" && (
        <>
          <div className="scrollbar-sleek flex shrink-0 gap-1 overflow-x-auto border-b px-3 py-2" role="group" aria-label="카드 고르기">
            {run.cards.map((c) => (
              <button
                key={c.id}
                type="button"
                aria-pressed={c.id === current?.id}
                onClick={() => onCard(c.id)}
                className="flex shrink-0 items-center gap-1.5 rounded-md border px-2 py-1 text-xs hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none aria-pressed:bg-accent"
              >
                <AgentStateDot state={c.state} />
                {c.label}
                <span className="text-muted-foreground">{PROVIDER_SHORT[c.provider]}</span>
              </button>
            ))}
          </div>
          {current && <CardTranscript card={current} names={names(run)} />}
        </>
      )}

      {run && view === "office" && <Office run={run} selected={current?.id} onSelect={(id) => { onCard(id); onView("work"); }} />}

      {run && view === "team" && <TeamView run={run} />}
    </section>
  );
}

function TeamView({ run }: { run: RunView }) {
  const name = names(run);
  return (
    <div className="scrollbar-sleek min-h-0 flex-1 overflow-y-auto bg-chat-canvas">
      <ol className="mx-auto flex max-w-2xl flex-col gap-3 px-5 py-5 text-sm text-chat-foreground">
        {run.messages.length === 0 && (
          <li className="text-chat-foreground-faint">
            아직 카드끼리 주고받은 메시지가 없습니다. 카드는 <code className="font-mono text-chat-code">o msg send</code>로 서로와 총괄에게 말할 수 있습니다.
          </li>
        )}
        {run.messages.map((m, i) => (
          <li key={`${m.at}-${i}`} className="rounded-lg border bg-card px-3.5 py-2.5 text-card-foreground">
            <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
              <span className="text-foreground">{name(m.from)}</span>
              <span aria-label="에게">→</span>
              <span className="text-foreground">{name(m.to)}</span>
              <time className="ml-auto" dateTime={m.at}>
                {timeOf(m.at)}
              </time>
            </p>
            <p className="mt-1 whitespace-pre-wrap">{m.text}</p>
          </li>
        ))}
      </ol>
    </div>
  );
}
