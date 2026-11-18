import { ChevronRight, OctagonX } from "lucide-react";
import { useEffect, useLayoutEffect, useRef } from "react";

import { blockedReason, failedBeforeCards, type RunView } from "../../../../src/core/snapshot.ts";
import { PROVIDER_NAME, STATE_WORD, clip, modelLine, names, timeOf } from "../../lib/format.ts";
import type { Message, ScreenState } from "../../data/useRun.ts";
import { AgentStateDot } from "../../ui/AgentStateDot.tsx";
import { Markdown } from "./Markdown.tsx";

interface Props {
  state: ScreenState;
  messages: Message[];
  run?: RunView;
  onOpenCard: (id: string) => void;
  onRetry: () => void;
  onCompose: () => void;
}

/** The lead chat, in Orca's native chat style: a centred column on the chat canvas. */
export function Thread({ state, messages, run, onOpenCard, onRetry, onCompose }: Props) {
  const scroller = useFollowBottom([messages, run]);
  const name = run ? names(run) : (party: string) => party;

  return (
    <div ref={scroller} className="scrollbar-sleek min-h-0 flex-1 overflow-y-auto bg-chat-canvas">
      <div className="mx-auto flex max-w-3xl flex-col gap-6 px-6 py-6 text-sm text-chat-foreground">
        {state === "loading" && (
          <div className="flex flex-col gap-4" aria-busy="true" aria-label="대화 불러오는 중">
            <div className="ml-auto h-16 w-3/5 rounded-lg bg-chat-user-surface" />
            <div className="h-24 w-4/5 rounded-lg bg-muted/40" />
          </div>
        )}
        {state === "error" && (
          <Notice title="기록을 읽지 못했습니다" action="다시 불러오기" onAction={onRetry}>
            실행 기록 파일(events.jsonl)을 열 수 없습니다. 터미널에서 <code className="font-mono text-chat-code">o status</code>로 실행이 남아 있는지 확인한 뒤 다시 불러오세요.
          </Notice>
        )}
        {(state === "empty" || (state === "full" && !run)) && (
          <Notice title="아직 맡긴 일이 없습니다" action="첫 요청 쓰기" onAction={onCompose}>
            총괄에게 할 일을 보내면 작업카드로 나뉘어 각 에이전트에게 전달됩니다.
          </Notice>
        )}
        {state === "full" &&
          messages.map((m) =>
            m.from === "me" ? (
              <div key={m.at} className="ml-auto max-w-[85%] rounded-lg border border-chat-user-border bg-chat-user-surface px-3.5 py-2.5">
                <Markdown text={m.text} />
              </div>
            ) : (
              <article key={m.at} aria-label="총괄의 답">
                <p className="mb-1.5 text-xs text-chat-foreground-faint">
                  총괄 Claude <time dateTime={m.at}>{timeOf(m.at)}</time>
                </p>
                <Markdown text={m.text} />
              </article>
            ),
          )}
        {state === "full" && run && <RunCard run={run} onOpenCard={onOpenCard} />}
        {state === "full" &&
          run?.messages
            .filter((m) => m.to === "lead" || m.from === "lead")
            .map((m, i) => (
              <article key={`${m.at}-${i}`} aria-label="팀 대화" className="rounded-lg border border-dashed px-3.5 py-2.5">
                <p className="mb-1 text-xs text-chat-foreground-faint">
                  {name(m.from)} → {name(m.to)} <time dateTime={m.at}>{timeOf(m.at)}</time>
                </p>
                <Markdown text={m.text} />
              </article>
            ))}
      </div>
    </div>
  );
}

const RUN_WORD: Record<RunView["status"], string> = { running: "진행 중", done: "모두 완료", failed: "실패한 카드 있음", stopped: "중지됨" };
const BLOCKED_WORD = "시작 전에 막힘";

/** Why a run ended before any card started: the runner's policy reasons (`run.blocked`), each with its rule. */
function BlockedReason({ run }: { run: RunView }) {
  return (
    <div role="note" aria-label="실행이 막힌 이유" className="border-b px-4 py-3 text-xs">
      <p className="flex items-center gap-1.5 font-medium text-destructive">
        <OctagonX className="size-3.5 shrink-0" aria-hidden="true" />
        카드를 하나도 시작하지 않았습니다.{run.blocked?.length ? " 실행 직전 정책 검사에서 막혔습니다." : ""}
      </p>
      {run.blocked?.length ? (
        <ul className="mt-1.5 space-y-1">
          {run.blocked.map((v, i) => (
            <li key={i} className="flex gap-2">
              <span className="shrink-0 font-mono text-destructive">[{v.rule}]</span>
              <span className="min-w-0 [overflow-wrap:anywhere]">
                {v.cardId && <span className="font-mono">{v.cardId}: </span>}
                {v.message}
              </span>
            </li>
          ))}
        </ul>
      ) : (
        <p className="mt-1.5 text-muted-foreground">{blockedReason(run)}</p>
      )}
    </div>
  );
}

export function RunCard({ run, onOpenCard }: { run: RunView; onOpenCard: (id: string) => void }) {
  const blocked = failedBeforeCards(run);
  return (
    <section className="overflow-hidden rounded-xl border bg-card text-card-foreground" aria-labelledby="run-title">
      <header className="flex items-center justify-between gap-3 border-b px-4 py-3">
        <h2 id="run-title" className="truncate text-sm font-medium">
          {run.title}
        </h2>
        <span role="status" className={`shrink-0 rounded-full border px-2 py-0.5 text-2xs ${blocked ? "border-destructive/40 text-destructive" : "text-muted-foreground"}`}>
          {blocked ? BLOCKED_WORD : RUN_WORD[run.status]}
        </span>
      </header>
      {blocked && <BlockedReason run={run} />}
      <ul>
        {run.cards.map((card) => (
          <li key={card.id} className="border-b last:border-b-0">
            <button
              type="button"
              onClick={() => onOpenCard(card.id)}
              className="grid w-full grid-cols-[auto_minmax(0,11rem)_minmax(0,1fr)_auto] items-center gap-3 px-4 py-2.5 max-sm:grid-cols-[auto_minmax(0,1fr)_auto] text-left hover:bg-accent/40 focus-ring focus-visible:ring-inset"
            >
              <AgentStateDot state={card.state} />
              <span className="min-w-0">
                <span className="block truncate text-sm">{card.label}</span>
                <span className="block truncate font-mono text-xs text-muted-foreground">
                  {PROVIDER_NAME[card.provider]} {modelLine(card.model, card.effort)}
                </span>
              </span>
              <span className="min-w-0 truncate text-xs text-muted-foreground max-sm:hidden" title={card.activity}>
                <span className="mr-1.5 text-foreground/80">{STATE_WORD[card.state]}</span>
                {clip(card.activity, 80)}
              </span>
              <ChevronRight className="size-4 text-muted-foreground" aria-hidden="true" />
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

export function Notice({ title, action, onAction, children }: { title: string; action: string; onAction: () => void; children: React.ReactNode }) {
  return (
    <section className="rounded-xl border bg-card p-6 text-card-foreground">
      <h2 className="text-sm font-medium">{title}</h2>
      <p className="mt-1.5 text-sm text-muted-foreground">{children}</p>
      <button
        type="button"
        onClick={onAction}
        className="mt-4 inline-flex h-8 items-center rounded-md bg-primary px-3 text-sm font-medium text-primary-foreground hover:bg-primary/90 focus-ring"
      >
        {action}
      </button>
    </section>
  );
}

/** Keeps the newest message in view, unless the reader has scrolled up to read. */
export function useFollowBottom(deps: unknown[]) {
  const ref = useRef<HTMLDivElement>(null);
  const follow = useRef(true);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const onScroll = () => {
      follow.current = el.scrollHeight - el.scrollTop - el.clientHeight <= 2;
    };
    el.addEventListener("scroll", onScroll, { passive: true });
    const content = el.firstElementChild;
    const resize = new ResizeObserver(() => { if (follow.current) el.scrollTop = el.scrollHeight; });
    if (content) resize.observe(content);
    return () => { el.removeEventListener("scroll", onScroll); resize.disconnect(); };
  }, []);
  useLayoutEffect(() => {
    const el = ref.current;
    if (el && follow.current) el.scrollTop = el.scrollHeight;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  return ref;
}
