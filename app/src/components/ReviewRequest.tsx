import { Bot, LoaderCircle } from "lucide-react";
import { useId, useState } from "react";

import type { CardView } from "../../../src/core/snapshot.ts";
import { PR_SELECTOR, backend } from "../backend.ts";
import { useSettings } from "../settings.ts";

/** The same machine side App.tsx uses: the recorded demo answers with a fixed refusal. */
const api = backend(new URLSearchParams(window.location.search).get("source") === "demo");

/** A card `o review` started (id and label as crates/o-core/src/review.rs sets them). */
export function isReviewCard(card: CardView): boolean {
  return card.id === "review" && card.role === "PR 검토";
}

type Status = { kind: "idle" } | { kind: "busy" } | { kind: "ok" | "error"; text: string };

/** Asks for one review and keeps what came back for the person to read. */
function useReviewRequest(project: string) {
  const [status, setStatus] = useState<Status>({ kind: "idle" });
  // Settings → 깨끗한 에이전트: the review card runs on the same profile as the rest of the app.
  const { clean } = useSettings();
  const request = async (pr: string) => {
    setStatus({ kind: "busy" });
    try {
      const started = await api.review(project, pr, clean);
      setStatus({ kind: started.ok ? "ok" : "error", text: started.message });
    } catch (e) {
      setStatus({ kind: "error", text: e instanceof Error ? e.message : String(e) });
    }
  };
  return { status, request };
}

function StatusLine({ status }: { status: Status }) {
  const text = status.kind === "busy" ? "PR을 읽고 검토 카드를 보내는 중…" : status.kind === "idle" ? "" : status.text;
  return (
    <span role="status" className={`min-w-0 text-2xs [overflow-wrap:anywhere] ${status.kind === "error" ? "text-red-400" : "text-muted-foreground"}`}>
      {text}
    </span>
  );
}

const BUTTON = "inline-flex h-6 shrink-0 items-center gap-1 rounded-md border px-2 text-2xs font-medium hover:bg-accent focus-ring disabled:opacity-60";

/**
 * "에이전트 검토 요청" for one PR: an agent other than the author reviews it and
 * comments on the PR (`o review <pr> --post`). It never approves or merges.
 */
export function ReviewButton({ project, pr }: { project: string; pr: string }) {
  const { status, request } = useReviewRequest(project);
  const busy = status.kind === "busy";
  return (
    <span className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
      <button type="button" disabled={busy || status.kind === "ok"} onClick={() => void request(pr)} className={BUTTON} title={`o review ${pr} --post`}>
        {busy ? <LoaderCircle className="size-3 animate-spin" aria-hidden="true" /> : <Bot className="size-3" aria-hidden="true" />}
        에이전트 검토 요청
      </button>
      <StatusLine status={status} />
    </span>
  );
}

/** For PRs people opened: type the number (or address) and an agent reviews it in this project's folder. */
export function ReviewRequestForm({ project }: { project: string }) {
  const id = useId();
  const [pr, setPr] = useState("");
  const { status, request } = useReviewRequest(project);
  const value = pr.trim().replace(/\/$/, "");
  const valid = PR_SELECTOR.test(value);
  return (
    <form
      className="mb-4 flex flex-col gap-1.5 rounded-md border bg-card p-3"
      onSubmit={(e) => {
        e.preventDefault();
        if (valid && status.kind !== "busy") void request(value);
      }}
    >
      <label htmlFor={id} className="text-xs font-medium">
        사람이 올린 PR을 에이전트에게 검토 맡기기
      </label>
      <span className="text-2xs text-muted-foreground">이 프로젝트 폴더의 저장소에서 gh로 PR을 읽고, 검토문을 PR 댓글로 남깁니다. 승인·병합은 하지 않습니다.</span>
      <span className="flex items-center gap-2">
        <input
          id={id}
          value={pr}
          onChange={(e) => setPr(e.target.value)}
          placeholder="PR 번호 또는 주소"
          inputMode="text"
          autoComplete="off"
          aria-invalid={pr.trim() !== "" && !valid}
          className="h-7 w-full min-w-0 max-w-64 rounded-md border bg-background px-2 text-xs focus-ring aria-invalid:border-red-400"
        />
        <button type="submit" disabled={!valid || status.kind === "busy"} className={`${BUTTON} h-7`}>
          {status.kind === "busy" ? <LoaderCircle className="size-3 animate-spin" aria-hidden="true" /> : <Bot className="size-3" aria-hidden="true" />}
          에이전트 검토 요청
        </button>
      </span>
      <StatusLine status={status} />
    </form>
  );
}
