import { CircleAlert, CircleCheck, FilePen, FileText, Gauge, Globe, Hourglass, SquareTerminal, Wrench } from "lucide-react";

import type { CardLogEntry, CardView, RunView } from "../../../../src/core/snapshot.ts";
import { lookOf } from "../office/characters.ts";
import { PROVIDER_NAME, STATE_WORD, modelLine, timeOf } from "../../lib/format.ts";
import { AgentStateDot } from "../../ui/AgentStateDot.tsx";
import { Avatar } from "../office/Avatar.tsx";
import { Markdown } from "./Markdown.tsx";
import { useFollowBottom } from "./Thread.tsx";
import type { ReactNode } from "react";

const TOOL_ICON: Record<string, typeof Wrench> = { "명령 실행": SquareTerminal, "파일 읽기": FileText, "파일 수정": FilePen, "웹 검색": Globe };

/** One work card's session, read back as Orca shows an agent transcript. */
export function CardTranscript({ card, run, names }: { card: CardView; run: RunView; names: (party: string) => string }) {
  // Who wrote a team message, drawn as in the team chat: a card's own person, or the lead.
  const avatarOf = (party: string) => {
    const sender = run.cards.find((c) => c.id === party);
    return sender ? <Avatar speaker={sender.provider} look={lookOf(run, sender.id)} size="sm" /> : <Avatar speaker="lead" size="sm" />;
  };
  const scroller = useFollowBottom([card.log.length]);
  return (
    <div className="flex min-h-0 flex-1 flex-col bg-chat-canvas">
      <header className="flex shrink-0 items-center gap-3 border-b px-6 py-2.5">
        <AgentStateDot state={card.state} size="md" />
        <Avatar speaker={card.provider} look={lookOf(run, card.id)} size="sm" />
        <span className="text-sm font-medium">{card.label}</span>
        <span className="font-mono text-xs text-muted-foreground">
          {PROVIDER_NAME[card.provider]} {modelLine(card.model, card.effort)}
        </span>
        <span className="ml-auto text-xs text-muted-foreground">
          {STATE_WORD[card.state]}
          {card.startedAt && ` ${timeOf(card.startedAt)}`}
          {card.endedAt && `–${timeOf(card.endedAt)}`}
        </span>
      </header>
      <div ref={scroller} className="scrollbar-sleek min-h-0 flex-1 overflow-y-auto">
        <ol className="mx-auto flex max-w-3xl flex-col gap-1 px-6 py-5 text-sm text-chat-foreground">
          {card.log.length === 0 && <li className="text-chat-foreground-faint">아직 남긴 기록이 없습니다.</li>}
          {card.log.map((entry, i) => (
            <Entry key={`${entry.at}-${i}`} entry={entry} cardFailed={card.state === "failed"} names={names} avatarOf={avatarOf} echoes={repeatsLastMessage(card.log, i)} />
          ))}
        </ol>
      </div>
    </div>
  );
}

/** CLIs often end with the same text as their last message; the end block then needs only its verdict. */
function repeatsLastMessage(log: CardLogEntry[], index: number): boolean {
  if (log[index].kind !== "end") return false;
  const last = log.slice(0, index).reverse().find((l) => l.kind === "message");
  return last?.text.trim() === log[index].text.trim();
}

function Entry({
  entry,
  cardFailed,
  names,
  avatarOf,
  echoes,
}: {
  entry: CardLogEntry;
  cardFailed: boolean;
  names: (party: string) => string;
  avatarOf: (party: string) => ReactNode;
  echoes: boolean;
}) {
  if (entry.kind === "team" && entry.team) {
    return (
      <li className="my-1 flex items-start gap-2 rounded-lg border border-chat-user-border bg-chat-user-surface px-3 py-2">
        <span className="shrink-0">{avatarOf(entry.team.from)}</span>
        <div className="min-w-0">
          <p className="text-xs text-chat-foreground-faint">
            {names(entry.team.from)} → {names(entry.team.to)}
          </p>
          <p className="whitespace-pre-wrap">{entry.text}</p>
        </div>
      </li>
    );
  }
  if (entry.kind === "tool" && entry.tool) {
    const Icon = TOOL_ICON[entry.tool.word] ?? Wrench;
    return (
      <li className="flex min-w-0 items-center gap-2 rounded-md px-2 py-1 hover:bg-accent/20">
        <Icon className="size-3.5 shrink-0 text-chat-foreground-faint" aria-hidden="true" />
        <span className="shrink-0 text-xs text-chat-foreground-faint">{entry.tool.word}</span>
        <span className="min-w-0 truncate font-mono text-chat-code text-chat-foreground" title={entry.tool.detail}>
          {entry.tool.detail}
        </span>
        <time className="ml-auto shrink-0 text-2xs text-chat-foreground-faint" dateTime={entry.at}>
          {timeOf(entry.at)}
        </time>
      </li>
    );
  }
  if (entry.kind === "message") {
    return (
      <li className="my-2 px-2">
        <Markdown text={entry.text} />
      </li>
    );
  }
  const failed = entry.kind === "end" && cardFailed;
  const Icon = entry.kind === "end" ? (failed ? CircleAlert : CircleCheck) : entry.kind === "queued" ? Hourglass : entry.kind === "budget" ? Gauge : CircleAlert;
  const tone = entry.kind === "end" && !failed ? "text-emerald-500" : entry.kind === "queued" ? "text-chat-foreground-faint" : entry.kind === "budget" ? "text-amber-500" : "text-destructive";
  return (
    <li className="mt-3 flex items-start gap-2 rounded-lg border bg-card px-3 py-2.5 text-card-foreground">
      <Icon className={`mt-0.5 size-4 shrink-0 ${tone}`} aria-hidden="true" />
      <div className="min-w-0">{echoes ? <p>{failed ? "실패로 끝났습니다." : "완료했습니다."} 결과는 위 메시지와 같습니다.</p> : <Markdown text={entry.text} />}</div>
    </li>
  );
}
