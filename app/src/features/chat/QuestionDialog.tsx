import { Check, MessageCircleQuestion } from "lucide-react";
import { useEffect, useId, useMemo, useRef, useState } from "react";

import { isLeadProvider, type LeadItem } from "../../../../src/core/lead.ts";
import { EMPTY_ANSWER, answerText, askNotice, cardQuestions, isComplete, pendingLeadAsk, toggle, type Answer, type PendingAsk } from "../../../../src/core/questions.ts";
import type { RunView } from "../../../../src/core/snapshot.ts";
import type { Backend } from "../../platform/backend/index.ts";
import { settingsNow, useSettings } from "../../data/settings.ts";
import type { LeadState } from "../../data/useLead.ts";
import { BUTTON_PRIMARY, BUTTON_SECONDARY } from "../../ui/controls.tsx";
import { Markdown } from "./Markdown.tsx";

const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e));
/** Raised by the thread's "답하기" to bring a question set aside back. */
const OPEN_EVENT = "project-o:open-questions";

export function openQuestions(key?: string) {
  window.dispatchEvent(new CustomEvent(OPEN_EVENT, { detail: key }));
}

/** A question just came in while the window is not in front: a toast (if allowed) and a flashing taskbar button. */
async function alertPerson(api: Backend, ask: PendingAsk) {
  if (!settingsNow().notifyQuestions || document.hasFocus()) return;
  void api.attention().catch(() => {});
  if (typeof Notification === "undefined") return;
  try {
    const permission = Notification.permission === "default" ? await Notification.requestPermission() : Notification.permission;
    if (permission !== "granted") return;
    const { title, body } = askNotice(ask);
    new Notification(title, { body });
  } catch {
    // Some webviews only allow notifications from a service worker; the flashing button still tells.
  }
}

/** The message's agent for answering the lead: the one the person last wrote to it with. */
function lastChoice(items: readonly LeadItem[]) {
  const user = items.findLast((item) => item.kind === "user");
  return user?.kind === "user" ? user : undefined;
}

/**
 * Questions waiting on the person, from the project's lead and from any running card that asked
 * the lead, one at a time in a dialog. The lead's answer waits for the picks: they go as the
 * thread's next message. A card's go to that card as the lead's reply. "나중에" sets one aside
 * until it is asked for again ("답하기" in the thread) or a newer question comes.
 */
export function QuestionDialog({ api, lead, runs, live }: { api: Backend; lead: LeadState; runs: readonly RunView[]; live: boolean }) {
  const { clean } = useSettings();
  const leadAsk = live ? pendingLeadAsk(lead.items) : undefined;
  const cards = useMemo(() => (live ? cardQuestions(runs) : []), [runs, live]);
  const asks = useMemo(() => [...(leadAsk ? [leadAsk] : []), ...cards], [leadAsk?.key, cards]);
  const [later, setLater] = useState<ReadonlySet<string>>(new Set());
  // Sent and not yet gone from the list (the next poll drops it); failed ones come back.
  const [sent, setSent] = useState<ReadonlySet<string>>(new Set());
  const [failed, setFailed] = useState<{ key: string; error: string }>();
  const shown = asks.find((ask) => !later.has(ask.key) && !sent.has(ask.key));

  const told = useRef(new Set<string>());
  useEffect(() => {
    for (const ask of asks) {
      if (told.current.has(ask.key)) continue;
      told.current.add(ask.key);
      void alertPerson(api, ask);
    }
  }, [asks, api]);

  useEffect(() => {
    const open = (event: Event) => {
      const key = (event as CustomEvent<string | undefined>).detail;
      setLater((now) => new Set([...now].filter((k) => key !== undefined && k !== key)));
    };
    window.addEventListener(OPEN_EVENT, open);
    return () => window.removeEventListener(OPEN_EVENT, open);
  }, []);

  const submit = async (ask: PendingAsk, answers: Answer[]) => {
    const text = answerText(ask, answers);
    setFailed(undefined);
    setSent((now) => new Set(now).add(ask.key));
    const back = (e: unknown) => {
      setSent((now) => new Set([...now].filter((k) => k !== ask.key)));
      setFailed({ key: ask.key, error: errorText(e) });
    };
    if (ask.source.kind === "card") {
      if (ask.source.approval && ask.source.messageId) await api.leadAnswerApproval(ask.source.runId, ask.source.messageId, text).catch(back);
      else await api.reply(ask.source.runId, ask.source.cardId, text, ask.source.messageId).catch(back);
      return;
    }
    // The lead answers after the picks arrive; the dialog closes as soon as they are on record.
    // The agent that asked answers, with the model and effort the person last used with it.
    const provider = ask.source.provider;
    if (!isLeadProvider(provider)) return back(new Error(`${provider}에게는 답을 보낼 수 없습니다`));
    const choice = lastChoice(lead.items);
    const same = choice?.provider === provider;
    void lead.send({ provider, model: same ? choice.model : undefined, effort: same ? choice.effort : undefined, text, attachments: [], clean }).catch(back);
  };

  if (!shown) return null;
  return (
    <Dialog
      key={shown.key}
      ask={shown}
      waiting={asks.filter((a) => a.key !== shown.key && !sent.has(a.key)).length}
      busy={shown.source.kind === "lead" && lead.busy}
      error={failed?.key === shown.key ? failed.error : undefined}
      onLater={() => setLater((now) => new Set(now).add(shown.key))}
      onSubmit={(answers) => void submit(shown, answers)}
    />
  );
}

function Dialog({ ask, waiting, busy, error, onLater, onSubmit }: { ask: PendingAsk; waiting: number; busy: boolean; error?: string; onLater: () => void; onSubmit: (answers: Answer[]) => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const first = useRef<HTMLButtonElement>(null);
  const titleId = useId();
  const [answers, setAnswers] = useState<Answer[]>(() => ask.questions.map(() => EMPTY_ANSWER));
  const complete = isComplete(ask.questions, answers);
  const set = (i: number, change: (a: Answer) => Answer) => setAnswers((now) => now.map((a, j) => (j === i ? change(a) : a)));

  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    dialog.current?.showModal();
    first.current?.focus();
    return () => {
      dialog.current?.close();
      previous?.focus();
    };
  }, []);

  const who = ask.source.kind === "lead" ? "총괄이 묻습니다" : `${ask.source.cardLabel} 카드가 묻습니다`;
  const where = ask.source.kind === "card" ? ask.source.runTitle : "답을 모두 고르면 총괄이 그에 맞춰 답합니다";
  return (
    <dialog
      ref={dialog}
      aria-labelledby={titleId}
      onCancel={(event) => {
        event.preventDefault();
        onLater();
      }}
      className="m-auto max-h-[85vh] w-[min(36rem,92vw)] overflow-y-auto rounded-xl border bg-card p-5 text-card-foreground shadow-lg backdrop:bg-black/50"
    >
      <header className="flex items-start gap-2.5">
        <MessageCircleQuestion className="mt-0.5 size-5 shrink-0 text-callout-note" aria-hidden="true" />
        <div className="min-w-0">
          <h2 id={titleId} className="text-base font-semibold">{who}</h2>
          <p className="truncate text-xs text-muted-foreground" title={where}>{where}</p>
        </div>
        {waiting > 0 && <span className="ml-auto shrink-0 rounded-full border px-2 py-0.5 text-2xs text-muted-foreground">대기 {waiting}개</span>}
      </header>
      {ask.intro && <div className="mt-3 text-sm text-muted-foreground"><Markdown text={ask.intro} /></div>}
      <ol className="mt-4 space-y-5">
        {ask.questions.map((q, i) => {
          const answer = answers[i] ?? EMPTY_ANSWER;
          const groupId = `${titleId}-q${i}`;
          return (
            <li key={i}>
              <fieldset aria-labelledby={groupId}>
                <p id={groupId} className="text-sm font-medium">
                  {q.header && <span className="mr-1.5 rounded-sm bg-muted px-1.5 py-0.5 text-2xs font-normal text-muted-foreground">{q.header}</span>}
                  {q.question}
                  {q.multi && <span className="ml-1.5 text-2xs font-normal text-muted-foreground">(여러 개 선택)</span>}
                </p>
                {q.options.length > 0 && (
                  <div className="mt-2 grid gap-1.5" role={q.multi ? "group" : "radiogroup"} aria-labelledby={groupId}>
                    {q.options.map((o, k) => {
                      const on = answer.chosen.includes(o.label);
                      return (
                        <button
                          key={o.label}
                          ref={i === 0 && k === 0 ? first : undefined}
                          type="button"
                          role={q.multi ? "checkbox" : "radio"}
                          aria-checked={on}
                          onClick={() => set(i, (a) => toggle(a, o.label, q.multi))}
                          className={`flex items-start gap-2 rounded-lg border px-3 py-2 text-left text-sm focus-ring hover:bg-accent ${on ? "border-ring bg-accent" : ""}`}
                        >
                          <span className="min-w-0 flex-1">
                            <span className="block">{o.label}</span>
                            {o.description && <span className="mt-0.5 block text-xs text-muted-foreground">{o.description}</span>}
                          </span>
                          <Check className={`mt-0.5 size-4 shrink-0 ${on ? "" : "invisible"}`} aria-hidden="true" />
                        </button>
                      );
                    })}
                  </div>
                )}
                <label className="mt-2 block">
                  <span className="sr-only">{q.question} — 직접 답하기</span>
                  <input
                    value={answer.other}
                    onChange={(e) => set(i, (a) => ({ ...a, other: e.target.value }))}
                    placeholder={q.options.length > 0 ? "기타: 직접 적기" : "답을 적어 주세요"}
                    className="h-8 w-full rounded-md border bg-background px-2.5 text-sm outline-none focus-ring placeholder:text-muted-foreground"
                  />
                </label>
              </fieldset>
            </li>
          );
        })}
      </ol>
      {error && <p role="alert" className="mt-4 text-xs whitespace-pre-wrap text-destructive">답을 보내지 못했습니다: {error}</p>}
      <footer className="mt-5 flex flex-wrap items-center justify-end gap-2">
        {!complete && <p className="mr-auto text-xs text-muted-foreground">모든 질문에 답하면 보낼 수 있습니다.</p>}
        {busy && <p className="mr-auto text-xs text-muted-foreground">총괄이 다른 답을 쓰는 중입니다.</p>}
        <button type="button" className={BUTTON_SECONDARY} onClick={onLater}>나중에</button>
        <button type="button" className={BUTTON_PRIMARY} disabled={!complete || busy} onClick={() => onSubmit(answers)}>답 보내기</button>
      </footer>
    </dialog>
  );
}
