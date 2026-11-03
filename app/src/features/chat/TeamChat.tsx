import type { RunView } from "../../../../src/core/snapshot.ts";
import { lookOf } from "../office/characters.ts";
import { names, timeOf } from "../../lib/format.ts";
import { Avatar, type Speaker } from "../office/Avatar.tsx";
import { Markdown } from "./Markdown.tsx";

const deliveryWords: Record<string, string> = { stored: "저장됨", delivered: "전달됨", unknown: "처리 여부 불명", acknowledged: "처리 확인", cancelled: "취소됨" };

/** The team channel as a messenger: each card speaks as its office character; the lead talks from the right. */
export function TeamChat({ run }: { run: RunView }) {
  const name = names(run);
  const isCard = (party: string) => party !== "lead" && run.cards.some((c) => c.id === party);
  const speaker = (party: string): Speaker => (party === "lead" ? "lead" : run.cards.find((c) => c.id === party)?.provider ?? "lead");

  return (
    <ol className="mx-auto flex flex-col gap-3 px-5 py-3 text-sm text-chat-foreground">
      {run.messages.length === 0 && (
        <li className="text-chat-foreground-faint">
          아직 카드끼리 주고받은 메시지가 없습니다. 카드는 <code className="font-mono text-chat-code">o msg send</code>로 서로와 총괄에게 말할 수 있습니다.
        </li>
      )}
      {run.messages.map((m, i) => {
        const mine = m.from === "lead";
        const grouped = i > 0 && run.messages[i - 1].from === m.from;
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
              {(m.kind === "question" || m.recipients) && <p className="mt-1 px-1 text-xs text-chat-foreground-faint">
                {m.kind === "question" && "질문"}
                {m.kind === "question" && m.recipients && " · "}
                {m.recipients && Object.entries(m.recipients).map(([recipient, state]) => `${name(recipient)}: ${deliveryWords[state] ?? state}`).join(" · ")}
              </p>}
            </div>
          </li>
        );
      })}
    </ol>
  );
}
