import { CircleAlert, CircleCheck, FilePen, FileText, Globe, Hourglass, SquareTerminal, Wrench } from "lucide-react";

import type { CardLogEntry, CardView } from "../../../src/core/snapshot.ts";
import { PROVIDER_NAME, STATE_WORD, modelLine, timeOf } from "../format.ts";
import { AgentStateDot } from "./AgentStateDot.tsx";
import { Markdown } from "./Markdown.tsx";
import { useFollowBottom } from "./Thread.tsx";

const TOOL_ICON: Record<string, typeof Wrench> = { "명령 실행": SquareTerminal, "파일 읽기": FileText, "파일 수정": FilePen, "웹 검색": Globe };

/** One work card's session, read back as Orca shows an agent transcript. */
export function CardTranscript({ card }: { card: CardView }) {
  const scroller = useFollowBottom([card.log.length]);
  return (
    <div className="flex min-h-0 flex-1 flex-col bg-chat-canvas">
      <header className="flex shrink-0 items-center gap-3 border-b px-6 py-2.5">
        <AgentStateDot state={card.state} size="md" />
        <span className="text-sm font-medium">{card.role}</span>
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
            <Entry key={`${entry.at}-${i}`} entry={entry} cardFailed={card.state === "failed"} />
          ))}
        </ol>
      </div>
    </div>
  );
}

function Entry({ entry, cardFailed }: { entry: CardLogEntry; cardFailed: boolean }) {
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
  const Icon = entry.kind === "end" ? (failed ? CircleAlert : CircleCheck) : entry.kind === "queued" ? Hourglass : CircleAlert;
  const tone = entry.kind === "end" && !failed ? "text-emerald-500" : entry.kind === "queued" ? "text-chat-foreground-faint" : "text-destructive";
  return (
    <li className="mt-3 flex items-start gap-2 rounded-lg border bg-card px-3 py-2.5 text-card-foreground">
      <Icon className={`mt-0.5 size-4 shrink-0 ${tone}`} aria-hidden="true" />
      <div className="min-w-0">
        <Markdown text={entry.text} />
      </div>
    </li>
  );
}
