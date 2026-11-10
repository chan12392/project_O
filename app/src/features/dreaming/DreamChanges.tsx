import { RotateCcw, Undo2, X } from "lucide-react";
import { useState } from "react";

import type { Backend, DreamChange } from "../../platform/backend/index.ts";

const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e));

const KIND_WORD: Record<DreamChange["kind"], string> = { "tool-failure": "되풀이된 도구 오류", "card-failure": "되풀이된 작업 실패" };
const STATE_WORD: Record<DreamChange["state"], string> = { applied: "적용 중", reverted: "되돌림", expired: "끝남" };

const when = (iso: string) => {
  const time = new Date(iso);
  return Number.isNaN(time.getTime()) ? iso : time.toLocaleString("ko-KR", { dateStyle: "short", timeStyle: "short" });
};

const folderName = (path: string) => path.split("/").filter(Boolean).pop() ?? path;

/**
 * What Dreaming changed (ADR-0041 개정 1): the lines O adds to a project's card prompts
 * because the same failure kept happening. They apply on their own; here a person reads
 * them and takes one back, or lets it apply again.
 */
export function DreamChanges({ api, changes, loadError, onChanged, onClose }: {
  api: Backend;
  changes: DreamChange[];
  /** The list could not be read: what is shown may be old, or empty for that reason. */
  loadError?: string;
  /** A change was taken back or restored: the list is read again. */
  onChanged: () => void;
  onClose: () => void;
}) {
  const [busy, setBusy] = useState<ReadonlySet<string>>(new Set());
  const [error, setError] = useState<string>();

  const set = (change: DreamChange, reverted: boolean) => {
    const mark = (on: boolean) =>
      setBusy((ids) => {
        const next = new Set(ids);
        if (on) next.add(change.id);
        else next.delete(change.id);
        return next;
      });
    mark(true);
    setError(undefined);
    api.dreamRevert(change.id, reverted).then(onChanged, (e) => setError(errorText(e))).finally(() => mark(false));
  };

  return (
    <aside className="absolute bottom-2 left-2 top-2 z-10 flex w-80 flex-col rounded-lg border bg-card text-xs shadow-lg" aria-label="Dreaming이 바꾼 것">
      <header className="flex shrink-0 items-start gap-2 border-b p-3">
        <div className="min-w-0 flex-1">
          <p className="font-medium">Dreaming이 바꾼 것</p>
          <p className="mt-1 leading-relaxed text-muted-foreground">같은 실패가 되풀이되면 그 프로젝트의 카드에게 한 줄로 알려 줍니다. 자동으로 적용되고, 여기에서 되돌릴 수 있습니다.</p>
        </div>
        <button type="button" onClick={onClose} aria-label="닫기" className="shrink-0 rounded focus-ring">
          <X className="size-3.5" aria-hidden="true" />
        </button>
      </header>
      {error && <p role="alert" className="shrink-0 border-b px-3 py-2 text-destructive">{error}</p>}
      {loadError && <p role="alert" className="shrink-0 border-b px-3 py-2 text-destructive">바꾼 것을 읽지 못했습니다: {loadError}</p>}
      {changes.length === 0 && loadError ? null : changes.length === 0 ? (
        <p className="p-3 leading-relaxed text-muted-foreground">아직 바꾼 것이 없습니다. 같은 도구 호출이 카드 둘에서(또는 세 번) 실패하거나, 같은 이유의 실패가 두 실행에서 나오면 여기에 생깁니다.</p>
      ) : (
        <ul className="min-h-0 flex-1 divide-y overflow-y-auto">
          {changes.map((change) => (
            <li key={change.id} className={`p-3 ${change.state === "applied" ? "" : "opacity-60"}`}>
              <p className="flex items-center gap-1.5 text-2xs text-muted-foreground">
                <span className="truncate font-mono" title={change.project}>{folderName(change.project)}</span>
                <span>· {KIND_WORD[change.kind]}</span>
                <span className={`ml-auto shrink-0 ${change.state === "applied" ? "text-agent-working" : ""}`}>{STATE_WORD[change.state]}</span>
              </p>
              <p className="mt-1.5 break-words leading-relaxed">{change.text}</p>
              <p className="mt-1.5 flex items-center gap-2 text-2xs text-muted-foreground">
                <span>마지막 {when(change.lastAt)}</span>
                <span title={change.evidence.map((e) => `${e.runId} · ${e.cardId}`).join("\n")}>근거 {change.evidence.length}곳</span>
                {change.state !== "expired" && (
                  <button type="button" disabled={busy.has(change.id)} onClick={() => set(change, change.state === "applied")} className="ml-auto flex items-center gap-1 rounded px-1.5 py-0.5 text-foreground hover:bg-sidebar-accent focus-ring disabled:opacity-50">
                    {change.state === "applied" ? <Undo2 className="size-3" aria-hidden="true" /> : <RotateCcw className="size-3" aria-hidden="true" />}
                    {change.state === "applied" ? "되돌리기" : "다시 적용"}
                  </button>
                )}
              </p>
            </li>
          ))}
        </ul>
      )}
    </aside>
  );
}
