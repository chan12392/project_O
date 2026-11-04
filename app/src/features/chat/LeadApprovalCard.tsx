import { Clock, ShieldQuestion } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { approvalView, detailIsLong, remainingText } from "../../../../src/core/leadWork.ts";
import { LEAD_NAME } from "../../../../src/core/lead.ts";
import type { LeadState } from "../../data/useLead.ts";
import { useSettings } from "../../data/settings.ts";
import { BUTTON_PRIMARY, BUTTON_SECONDARY } from "../../ui/controls.tsx";

const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e));
const NAMES: Record<string, string> = LEAD_NAME;

/** A shell command or a request as sent: in full, folded when long. */
function Detail({ text, label }: { text: string; label: string }) {
  if (!text.trim()) return null;
  const block = <pre className="scrollbar-sleek mt-1 max-h-48 overflow-auto rounded-md border bg-background/60 px-2.5 py-2 font-mono text-xs whitespace-pre-wrap [overflow-wrap:anywhere]">{text}</pre>;
  if (!detailIsLong(text)) return <div><p className="text-2xs text-muted-foreground">{label}</p>{block}</div>;
  return (
    <details>
      <summary className="cursor-pointer rounded text-2xs text-muted-foreground hover:text-foreground focus-ring">{label} 펼치기</summary>
      {block}
    </details>
  );
}

/**
 * The action a normal-mode lead waits on (ADR-0008 개정 3), right above the composer: what it
 * asks in full, how long it waits, and 거부 (focused first) or 허용. There is no "always allow":
 * each action is its own decision, and only these buttons decide, never text in the chat.
 */
export function LeadApprovalCard({ lead }: { lead: LeadState }) {
  const { clean } = useSettings();
  const [now, setNow] = useState(() => Date.now());
  const [state, setState] = useState<{ busy?: boolean; error?: string }>({});
  const deny = useRef<HTMLButtonElement>(null);
  const approvals = lead.thread?.busy ? lead.thread.approvals : undefined;
  const view = approvalView(approvals, now);
  const id = view?.id;
  useEffect(() => {
    if (!id) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [id]);
  // A new request takes the focus on 거부, the safe answer: Enter or Space refuses it.
  useEffect(() => {
    if (!id) return;
    setState({});
    deny.current?.focus();
  }, [id]);
  if (!view || !approvals) return null;
  const first = approvals[0];
  const decide = async (allow: boolean) => {
    setState({ busy: true });
    try {
      await lead.decide(view.id, allow);
      setState({});
    } catch (e) {
      setState({ error: errorText(e) });
    }
  };
  return (
    <section
      role="alertdialog"
      aria-modal="false"
      aria-labelledby={`${view.id}-title`}
      aria-describedby={`${view.id}-what`}
      className="mx-auto mb-2 max-w-3xl rounded-xl border border-status-warning-text/40 bg-card text-card-foreground shadow-sm"
    >
      <header className="flex items-start justify-between gap-3 px-3.5 pt-3">
        <div className="min-w-0">
          <h2 id={`${view.id}-title`} className="flex items-center gap-1.5 text-sm font-medium">
            <ShieldQuestion className="size-4 shrink-0 text-status-warning-text" aria-hidden="true" />
            {view.title} 승인 요청
          </h2>
          <p className="mt-0.5 text-2xs text-muted-foreground">
            총괄 {NAMES[first.provider] ?? first.provider}이 이 작업을 하려고 합니다{view.more > 0 ? ` · 뒤에 ${view.more}개 더 기다림` : ""}
          </p>
        </div>
        <p className="flex shrink-0 items-center gap-1 text-2xs text-muted-foreground tabular-nums" aria-live="off">
          <Clock className="size-3" aria-hidden="true" />
          {remainingText(view.secondsLeft)}
        </p>
      </header>
      <div id={`${view.id}-what`} className="space-y-2 px-3.5 py-2.5 text-xs">
        {view.files.length > 0 ? (
          <ul aria-label="바뀌는 파일" className="space-y-1">
            {view.files.map((file) => (
              <li key={file.path}>
                <details>
                  <summary className="flex cursor-pointer items-center gap-2 rounded focus-ring" title={file.path}>
                    <span className="min-w-0 truncate font-mono">'{file.name}'</span>
                    {file.counts && (
                      <span className="shrink-0 font-mono tabular-nums">
                        <span className="text-status-success">+{file.added}</span> <span className="text-destructive">−{file.removed}</span>
                      </span>
                    )}
                    <span className="min-w-0 truncate text-2xs text-muted-foreground">{file.path}</span>
                  </summary>
                  {file.diff && <pre className="scrollbar-sleek mt-1 max-h-56 overflow-auto rounded-md border bg-background/60 px-2.5 py-2 font-mono text-xs whitespace-pre">{file.diff}</pre>}
                </details>
              </li>
            ))}
          </ul>
        ) : (
          <p className="font-mono text-sm [overflow-wrap:anywhere]">{view.summary}</p>
        )}
        {first.kind === "command" && <Detail text={view.detail} label="실행할 명령 전체" />}
        {first.kind !== "command" && view.files.length === 0 && <Detail text={view.detail} label="총괄 요청 원문" />}
        {view.cwd && <p className="text-2xs text-muted-foreground">실행 위치 <span className="font-mono [overflow-wrap:anywhere]">{view.cwd}</span></p>}
        {view.reason && <p className="text-2xs text-muted-foreground">총괄이 적은 이유(참고): {view.reason}</p>}
        <p className="text-2xs text-muted-foreground">
          채팅에 적힌 '승인'은 결정이 아닙니다. 이 버튼으로만 허용됩니다. 응답이 없으면 거부합니다.
          {!clean && " 내 설정으로 실행 중이라 그 설정의 허용 규칙·플러그인이 먼저 적용될 수 있습니다."}
        </p>
      </div>
      <footer className="flex flex-wrap items-center gap-2 border-t px-3.5 py-2.5">
        <button ref={deny} type="button" onClick={() => void decide(false)} disabled={state.busy} className={BUTTON_SECONDARY}>거부</button>
        <button type="button" onClick={() => void decide(true)} disabled={state.busy} className={BUTTON_PRIMARY}>허용</button>
        <span className="text-2xs text-muted-foreground">이번 작업 한 번만 적용됩니다</span>
        {state.error && <p role="alert" className="w-full text-xs whitespace-pre-wrap text-destructive">{state.error}</p>}
      </footer>
    </section>
  );
}
