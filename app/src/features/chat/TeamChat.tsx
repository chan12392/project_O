import { useId, useState } from "react";
import type { RunView } from "../../../../src/core/snapshot.ts";
import { currentBackend } from "../../platform/backend/index.ts";
import { lookOf } from "../office/characters.ts";
import { errorText, names, timeOf } from "../../lib/format.ts";
import { Avatar, type Speaker } from "../office/Avatar.tsx";
import { Markdown } from "./Markdown.tsx";

const deliveryWords: Record<string, string> = { stored: "저장됨", delivered: "전달됨", unknown: "처리 여부 불명", acknowledged: "처리 확인", cancelled: "취소됨" };

function ApprovalAnswer({ runId, messageId }: { runId: string; messageId: string }) {
  const field = useId();
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [sent, setSent] = useState(false);
  const [error, setError] = useState("");
  const submit = async () => {
    if (busy || !text.trim()) return;
    setBusy(true); setError("");
    try {
      await currentBackend().leadAnswerApproval(runId, messageId, text);
      setSent(true);
    } catch (e) { setError(errorText(e)); }
    finally { setBusy(false); }
  };
  if (sent) return <p role="status" className="mt-2 text-xs text-chat-foreground-faint">답변을 보냈습니다.</p>;
  return <form className="mt-2 w-full space-y-2" onSubmit={(event) => { event.preventDefault(); void submit(); }}>
    <label htmlFor={field} className="block text-xs text-chat-foreground">승인 요청에 답하기</label>
    <textarea id={field} value={text} onChange={(event) => setText(event.target.value)} rows={2} maxLength={4000} className="w-full rounded border bg-transparent p-2 text-sm focus-ring" placeholder="판단과 필요한 조건을 적어 주세요" />
    {error && <p role="alert" className="whitespace-pre-wrap text-xs">{error}</p>}
    <button type="submit" disabled={busy || !text.trim()} className="rounded border px-2 py-1 text-xs focus-ring disabled:opacity-50">{busy ? "보내는 중…" : "답변 보내기"}</button>
  </form>;
}

/** The team channel as a messenger: each card speaks as its office character; the lead talks from the right. */
export function TeamChat({ run }: { run: RunView }) {
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const wake = async () => {
    setBusy(true); setNotice("");
    try {
      const result = await currentBackend().leadWake(run.runId);
      setNotice(result.processed ? "대기 메시지를 총괄이 처리했습니다." : "현재 총괄에 전달할 대기 메시지가 없습니다.");
    } catch (e) { setNotice(errorText(e)); }
    finally { setBusy(false); }
  };
  const retry = async (messageId: string) => {
    setBusy(true); setNotice("");
    try {
      const result = await currentBackend().leadRetry(run.runId, messageId);
      setNotice(result.state === "queued" ? "다시 전달할 수 있도록 대기 상태로 바꿨습니다." : "기존 답변을 확인해 처리 완료로 바꿨습니다.");
    } catch (e) { setNotice(errorText(e)); }
    finally { setBusy(false); }
  };
  const name = names(run);
  const isCard = (party: string) => party !== "lead" && run.cards.some((c) => c.id === party);
  const speaker = (party: string): Speaker => (party === "lead" ? "lead" : run.cards.find((c) => c.id === party)?.provider ?? "lead");

  return (
    <ol className="mx-auto flex flex-col gap-3 px-5 py-3 text-sm text-chat-foreground">
      {run.messages.some((m) => m.kind !== "approval" && m.from !== "lead" && (m.to === "lead" || m.to === "all")) && <li><button type="button" disabled={busy} onClick={() => void wake()} className="rounded border px-2 py-1 text-xs focus-ring disabled:opacity-50">{busy ? "총괄이 확인 중…" : "대기 메시지 총괄에 전달"}</button>{notice && <p role="status" className="mt-1 whitespace-pre-wrap text-xs text-chat-foreground-faint">{notice}</p>}</li>}
      {run.messages.length === 0 && (
        <li className="text-chat-foreground-faint">
          아직 카드끼리 주고받은 메시지가 없습니다. 카드는 <code className="font-mono text-chat-code">o msg send</code>로 서로와 총괄에게 말할 수 있습니다.
        </li>
      )}
      {run.messages.map((m, i) => {
        const mine = m.from === "lead";
        const grouped = i > 0 && run.messages[i - 1].from === m.from;
        const answered = m.id && run.messages.some((reply) => reply.from === "lead" && reply.to === m.from && reply.replyTo === m.id);
        return (
          <li key={`${m.at}-${i}`} className={`flex items-end gap-2 ${mine ? "flex-row-reverse" : ""} ${grouped ? "-mt-1.5" : ""}`}>
            <span className={grouped ? "invisible" : ""}>
              <Avatar speaker={speaker(m.from)} look={isCard(m.from) ? lookOf(run, m.from) : undefined} />
            </span>
            <div className={`flex max-w-[78%] min-w-0 flex-col ${mine ? "items-end" : "items-start"}`}>
              {!grouped && (
                <p className="mb-1 px-1 text-xs text-chat-foreground-faint">
                  <span className="text-chat-foreground">{name(m.from)}</span>
                  {m.to !== "all" && <span> → {name(m.to)}</span>}
                  <time className="ml-1.5" dateTime={m.at}>
                    {timeOf(m.at)}
                  </time>
                </p>
              )}
              <div
                className={`max-w-full min-w-0 rounded-2xl px-3.5 py-2 ${
                  mine ? "rounded-br-md bg-primary text-primary-foreground" : "rounded-bl-md border border-chat-user-border bg-chat-user-surface"
                }`}
              >
                <Markdown text={m.text} inverse={mine} />
              </div>
              {(m.kind === "question" || m.kind === "approval" || m.replyTo || m.recipients) && <p className="mt-1 break-all px-1 text-xs text-chat-foreground-faint">
                {m.kind === "question" && `질문${m.id ? ` (${m.id})` : ""}`}
                {m.kind === "approval" && `사용자 승인 요청${m.id ? ` (${m.id})` : ""} · ${answered ? "답변 완료" : "직접 확인 필요"}`}
                {m.replyTo && `질문 ${m.replyTo}에 대한 답변`}
                {(m.kind === "question" || m.kind === "approval" || m.replyTo) && m.recipients && " · "}
                {m.recipients && Object.entries(m.recipients).map(([recipient, state]) => `${name(recipient)}: ${deliveryWords[state] ?? state}`).join(" · ")}
              </p>}
              {m.kind === "approval" && m.id && m.to === "lead" && !answered && m.recipients?.lead !== "cancelled" && <ApprovalAnswer key={`${run.runId}-${m.id}`} runId={run.runId} messageId={m.id} />}
              {m.kind !== "approval" && m.id && (m.recipients?.lead === "unknown" || m.recipients?.lead === "delivered") && <div className="mt-1 text-xs text-chat-foreground-faint"><p>이전 답변을 확인한 후 복구해 주세요. 재전송은 총괄을 다시 실행할 수 있습니다.</p><button type="button" disabled={busy} onClick={() => void retry(m.id!)} className="mt-1 rounded border px-2 py-1 focus-ring disabled:opacity-50">메시지 상태 복구</button></div>}
            </div>
          </li>
        );
      })}
    </ol>
  );
}
