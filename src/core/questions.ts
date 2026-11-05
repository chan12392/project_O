/**
 * Questions that wait on the person, from the lead and from cards, as one list the app asks
 * in a dialog (pure, like attention.ts). The lead asks with an `o-ask` block (`lead.ask`);
 * a card asks the lead on the team channel, its choices after "[보기:" (the card prompt,
 * crates/o-core/src/providers `CHOICES_OPEN`). Answers go back the way the question came:
 * the lead's as the next message of its thread, a card's as the lead's reply to that card.
 */
import type { AskQuestion, LeadItem } from "./lead.ts";
import type { ProviderId } from "./plan.ts";
import type { RunView } from "./snapshot.ts";
import { cancelledForLead, isSessionRun } from "./attention.ts";

export interface Choice {
  label: string;
  description?: string;
}

export interface Question {
  header?: string;
  question: string;
  /** None: the person answers in their own words. */
  options: Choice[];
  multi: boolean;
}

export type AskSource =
  | { kind: "lead"; askId: string; provider: ProviderId }
  | { kind: "card"; runId: string; runTitle: string; cardId: string; cardLabel: string; provider?: ProviderId; messageId?: string; approval?: boolean };

export interface PendingAsk {
  /** Stable while the question waits; a newer question gets another. */
  key: string;
  source: AskSource;
  intro: string;
  questions: Question[];
  since: string;
}

/** One question's answer: the options picked and the person's own words ("기타"). */
export interface Answer {
  chosen: string[];
  other: string;
}

export const EMPTY_ANSWER: Answer = { chosen: [], other: "" };

/** The lead's latest questions while nothing has been sent after them. */
export function pendingLeadAsk(items: readonly LeadItem[]): PendingAsk | undefined {
  const last = items.findLast((item) => item.kind === "ask");
  if (!last || last.kind !== "ask" || last.answered) return undefined;
  return {
    key: `lead:${last.askId}`,
    source: { kind: "lead", askId: last.askId, provider: last.provider },
    intro: last.intro,
    questions: last.questions.map(fromAsk),
    since: last.at,
  };
}

function fromAsk(q: AskQuestion): Question {
  return { header: q.header, question: q.question, options: q.options.map((o) => ({ label: o.label, description: o.description })), multi: Boolean(q.multi) };
}

/** A message to the lead asks something: a question mark, "~할까요", "~주세요", or choices. */
const ASKS = /[?？]|까요|할지|주세요|부탁|알려/;
const CHOICES = /\[\s*보기\s*:\s*([^\]]*)\]\s*$/;

/** A card's message as a question: its text without the choices, and the choices "[보기: A | B]" lists. */
export function parseCardQuestion(text: string): Question {
  const match = CHOICES.exec(text.trim());
  const labels = match ? [...new Set(match[1].split("|").map((s) => s.trim()).filter(Boolean))].slice(0, 6) : [];
  const question = (match ? text.trim().slice(0, match.index) : text).trim();
  return { question: question || text.trim(), options: labels.length >= 2 ? labels.map((label) => ({ label })) : [], multi: false };
}

/**
 * Card questions the lead has not answered, from O runs still running (a finished card could
 * not read the answer) and cards still at work. Messages that only report are left out.
 */
export function cardQuestions(runs: readonly RunView[]): PendingAsk[] {
  const asks: PendingAsk[] = [];
  for (const run of runs) {
    if (isSessionRun(run) || run.status !== "running") continue;
    const answeredAt = (party: string) => run.messages.findLast((m) => m.from === "lead" && (m.to === party || m.to === "all"))?.at ?? "";
    for (const card of run.cards) {
      if (card.state !== "running" && card.state !== "waiting") continue;
      // A `--question` goes to the lead, which answers it itself (ADR-0029); only an approval waits on the person.
      const structured = run.messages.filter((m) => m.from === card.id && m.to === "lead" && m.id && m.kind === "approval" && !cancelledForLead(m) && !run.messages.some((reply) => reply.from === "lead" && reply.replyTo === m.id));
      for (const m of structured) {
        asks.push({ key: `card:${run.runId}:${card.id}:${m.id}`, source: { kind: "card", runId: run.runId, runTitle: run.title, cardId: card.id, cardLabel: card.label, provider: card.provider, messageId: m.id, approval: m.kind === "approval" }, intro: "", questions: [parseCardQuestion(m.text)], since: m.at });
      }
      const open = run.messages.filter((m) => m.from === card.id && m.to === "lead" && m.kind !== "question" && m.kind !== "approval" && m.at > answeredAt(card.id) && (ASKS.test(m.text) || CHOICES.test(m.text)));
      if (open.length === 0) continue;
      asks.push({
        key: `card:${run.runId}:${card.id}:${open[0].at}`,
        source: { kind: "card", runId: run.runId, runTitle: run.title, cardId: card.id, cardLabel: card.label, provider: card.provider },
        intro: "",
        questions: open.slice(-4).map((m) => parseCardQuestion(m.text)),
        since: open[0].at,
      });
    }
  }
  return asks.sort((a, b) => a.since.localeCompare(b.since));
}

/** Every question has an answer: a pick, or the person's own words. */
export function isComplete(questions: readonly Question[], picks: readonly Answer[]): boolean {
  return questions.every((_, i) => (picks[i]?.chosen.length ?? 0) > 0 || Boolean(picks[i]?.other.trim()));
}

/** A question picked for: one option replaces the last unless it takes several. */
export function toggle(pick: Answer, label: string, multi: boolean): Answer {
  if (!multi) return { ...pick, chosen: pick.chosen[0] === label ? [] : [label] };
  return { ...pick, chosen: pick.chosen.includes(label) ? pick.chosen.filter((c) => c !== label) : [...pick.chosen, label] };
}

function answerOf(pick: Answer): string {
  return [...pick.chosen, ...(pick.other.trim() ? [pick.other.trim()] : [])].join(", ");
}

/** What goes back: the lead gets every question with its answer; a card gets the same in one message. */
export function answerText(ask: PendingAsk, picks: readonly Answer[]): string {
  const lines = ask.questions.map((q, i) => `- ${q.header || q.question}: ${answerOf(picks[i] ?? EMPTY_ANSWER)}`);
  if (ask.source.kind === "card" && ask.questions.length === 1) return `답: ${answerOf(picks[0] ?? EMPTY_ANSWER)}`;
  return [ask.source.kind === "lead" ? "[질문 답변]" : "답:", ...lines].join("\n");
}

/** The notification for a question that just came in. */
export function askNotice(ask: PendingAsk): { title: string; body: string } {
  const first = ask.questions[0]?.question ?? "";
  const more = ask.questions.length > 1 ? ` 외 ${ask.questions.length - 1}개` : "";
  const who = ask.source.kind === "lead" ? "총괄" : `${ask.source.cardLabel} 카드`;
  return { title: `${who}의 질문`, body: `${first}${more}` };
}
