import { BookOpen, CircleAlert, CircleCheck, FilePen, FileText, Gauge, Globe, Hourglass, SquareTerminal, Wrench } from "lucide-react";

import type { CardLogEntry, CardView, RunView } from "../../../../src/core/snapshot.ts";
import { EMPTY_COMPOSER, type Composer } from "../../../../src/core/panes.ts";
import { lookOf } from "../office/characters.ts";
import { PROVIDER_NAME, STATE_WORD, errorText, modelLine, timeOf } from "../../lib/format.ts";
import { AgentStateDot } from "../../ui/AgentStateDot.tsx";
import { Avatar } from "../office/Avatar.tsx";
import { Markdown } from "./Markdown.tsx";
import { ImageScope } from "./ImageGallery.tsx";
import { prOf } from "../../../../src/core/attention.ts";
import { ExternalAnchor } from "../../ui/ExternalAnchor.tsx";
import { ReviewButton, isReviewCard } from "../work/ReviewRequest.tsx";
import { useFollowBottom } from "./Thread.tsx";
import { useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { useAutoGrow } from "../../lib/autoGrow.ts";
import type { Backend } from "../../platform/backend/index.ts";
import { sessionForCard } from "../../../../src/core/terminal.ts";
import { TerminalView } from "../terminal/TerminalView.tsx";
import { useTermSessions } from "../terminal/useTermSessions.ts";

const TOOL_ICON: Record<string, typeof Wrench> = { "명령 실행": SquareTerminal, "파일 읽기": FileText, "파일 수정": FilePen, "웹 검색": Globe };

/** One work card's session, read back as Orca shows an agent transcript. */
export function CardTranscript({ card, run, names, api, onFollowup, composer }: { card: CardView; run: RunView; names: (party: string) => string; api?: Backend; onFollowup?: (runId: string) => void; composer?: Composer & { set: (change: Partial<Composer>) => void } }) {
  // The draft lives with the caller when it gives one, so hiding this transcript keeps it.
  const [own, setOwn] = useState<Composer>(EMPTY_COMPOSER);
  const { draft, sending } = composer ?? own;
  const update = composer?.set ?? ((change: Partial<Composer>) => setOwn((current) => ({ ...current, ...change })));
  const setDraft = (text: string) => update({ draft: text });
  const setSending = (value: boolean) => update({ sending: value });
  const draftRef = useRef<HTMLTextAreaElement>(null);
  useAutoGrow(draftRef, draft);
  const [error, setError] = useState<string>();
  const [notice, setNotice] = useState<string>();
  const send = async () => {
    if (!api || !onFollowup || !draft.trim() || sending) return;
    setSending(true);
    setError(undefined);
    setNotice(undefined);
    try {
      const result = await api.followup(run.runId, card.id, draft);
      setDraft("");
      setNotice("후속 지시를 접수했습니다. 작업 상태가 곧 표시됩니다.");
      onFollowup(result.runId);
    } catch (e) { setError(errorText(e)); }
    finally { setSending(false); }
  };
  // As the lead composer: Ctrl+Enter inserts a line break, Shift+Enter keeps the default one, Enter sends.
  const onKey = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key !== "Enter" || event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229) return;
    if (event.ctrlKey && !event.altKey && !event.metaKey) {
      event.preventDefault();
      const input = event.currentTarget;
      input.setRangeText("\n", input.selectionStart, input.selectionEnd, "end");
      setDraft(input.value);
    } else if (!event.shiftKey && !event.ctrlKey && !event.altKey && !event.metaKey) {
      event.preventDefault();
      if (!event.repeat) void send();
    }
  };
  // Who wrote a team message, drawn as in the team chat: a card's own person, or the lead.
  const avatarOf = (party: string) => {
    const sender = run.cards.find((c) => c.id === party);
    return sender ? <Avatar speaker={sender.provider} look={lookOf(run, sender.id)} size="sm" /> : <Avatar speaker="lead" size="sm" />;
  };
  const scroller = useFollowBottom([card.log.length]);
  const pr = prOf(card);
  // A card that runs as a terminal shows its session above the transcript (ADR-0042).
  const { sessions: termSessions } = useTermSessions(api, Boolean(api));
  const termSession = sessionForCard(termSessions, run.runId, card.id);
  // Where follow-ups can't go, one line says why instead of a disabled input.
  const readOnly = card.provider === "agy" ? "터미널 카드는 후속 지시를 받지 않습니다" : run.runId.startsWith("session-") ? "외부 세션은 기록만 볼 수 있습니다" : undefined;
  return (
    <ImageScope.Provider value={{ project: run.project, run: run.runId, card: card.id }}><div className="flex min-h-0 flex-1 flex-col bg-chat-canvas">
      <header className="flex min-w-0 shrink-0 items-center gap-3 border-b px-6 py-2.5">
        <AgentStateDot state={card.state} size="md" />
        <Avatar speaker={card.provider} look={lookOf(run, card.id)} size="sm" />
        <span className="min-w-0 truncate text-sm font-medium" title={card.label}>{card.label}</span>
        <div className="min-w-0 text-xs text-muted-foreground">
          <p className="truncate">요청: {PROVIDER_NAME[card.provider]} {modelLine(card.model, card.effort)}</p>
          <p className="truncate" title={card.actualModels?.join(", ")}>실제: {card.actualModels?.join(", ") || "모델 미확인"}{card.usageComplete === false && " · 사용량 일부 확인"}</p>
        </div>
        <span className="ml-auto shrink-0 text-xs whitespace-nowrap text-muted-foreground">
          {STATE_WORD[card.state]}
          {card.startedAt && ` ${timeOf(card.startedAt)}`}
          {card.endedAt && `–${timeOf(card.endedAt)}`}
        </span>
      </header>
      {pr && <div className="flex flex-wrap items-center gap-3 border-b px-6 py-2 text-xs"><ExternalAnchor href={pr} className="text-callout-note underline focus-ring">{pr}</ExternalAnchor>{!isReviewCard(card) && <ReviewButton project={run.project} pr={pr} />}</div>}
      {api && termSession && <div className="h-64 shrink-0 border-b"><TerminalView api={api} session={termSession} title={`${card.label} 터미널`} /></div>}
      <div ref={scroller} className="scrollbar-sleek min-h-0 flex-1 overflow-y-auto">
        <ol className="mx-auto flex max-w-3xl flex-col gap-1 px-6 py-5 text-sm text-chat-foreground">
          {card.log.length === 0 && <li className="text-chat-foreground-faint">아직 남긴 기록이 없습니다.</li>}
          {card.log.map((entry, i) => (
            <Entry key={`${entry.at}-${i}`} entry={entry} cardFailed={card.state === "failed"} names={names} avatarOf={avatarOf} echoes={repeatsLastMessage(card.log, i)} />
          ))}
        </ol>
      </div>
      {api && onFollowup && (readOnly ? (
        <p className="shrink-0 border-t px-4 py-2 text-xs text-chat-foreground-faint">{readOnly}</p>
      ) : (
        <form className="shrink-0 border-t p-3" onSubmit={(event) => { event.preventDefault(); void send(); }}>
          <label className="sr-only" htmlFor={`followup-${run.runId}-${card.id}`}>{card.label}에게 후속 지시</label>
          <div className="flex items-end gap-2">
            <textarea ref={draftRef} id={`followup-${run.runId}-${card.id}`} value={draft} onChange={(event) => setDraft(event.target.value)} onKeyDown={onKey} disabled={sending} placeholder={card.state === "running" || card.state === "queued" || card.state === "waiting" ? "현재 작업이 끝나면 전달합니다. 이미 보낸 지시가 대기·실행 중이면 그 지시가 끝난 뒤 보내세요" : "후속 지시를 입력하세요"} className="scrollbar-sleek max-h-48 min-w-0 flex-1 resize-none overflow-y-auto rounded-md border bg-background px-3 py-2 text-sm focus-ring disabled:opacity-50" rows={2} />
            <button type="submit" disabled={sending || !draft.trim()} className="shrink-0 rounded-md border px-2 py-1 text-xs text-muted-foreground hover:bg-accent focus-ring disabled:opacity-50">{sending ? "보내는 중…" : "보내기"}</button>
          </div>
          {error && <p role="alert" className="mt-1 text-xs text-destructive">{error}</p>}
          {notice && <p role="status" className="mt-1 text-xs text-muted-foreground">{notice}</p>}
        </form>
      ))}
    </div></ImageScope.Provider>
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
          <Markdown text={entry.text} />
        </div>
      </li>
    );
  }
  if (entry.kind === "tool" && entry.tool) {
    if (entry.tool.word === "O 작업 폴더") {
      return <li className="min-w-0 px-2 py-1"><span className="text-xs text-chat-foreground-faint">작업 폴더</span><p className="mt-1 select-text break-all font-mono text-chat-code">{entry.tool.detail}</p></li>;
    }
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
  if (entry.kind === "input") return <li className="my-2 ml-auto max-w-[90%] rounded-lg border border-chat-user-border bg-chat-user-surface px-3 py-2"><Markdown text={entry.text} /></li>;
  if (entry.kind === "message") {
    return (
      <li className="my-2 px-2">
        <Markdown text={entry.text} />
      </li>
    );
  }
  const failed = entry.kind === "end" && cardFailed;
  const Icon = entry.kind === "end" ? (failed ? CircleAlert : CircleCheck) : entry.kind === "queued" ? Hourglass : entry.kind === "budget" ? Gauge : entry.kind === "guides" ? BookOpen : CircleAlert;
  const tone = entry.kind === "end" && !failed ? "text-emerald-500" : entry.kind === "queued" || entry.kind === "guides" || entry.kind === "fallback" ? "text-chat-foreground-faint" : entry.kind === "budget" ? "text-amber-500" : "text-destructive";
  return (
    <li className="mt-3 flex items-start gap-2 rounded-lg border bg-card px-3 py-2.5 text-card-foreground">
      <Icon className={`mt-0.5 size-4 shrink-0 ${tone}`} aria-hidden="true" />
      <div className="min-w-0">{echoes ? <p>{failed ? "실패로 끝났습니다." : "완료했습니다."} 결과는 위 메시지와 같습니다.</p> : <Markdown text={entry.text} />}</div>
    </li>
  );
}
