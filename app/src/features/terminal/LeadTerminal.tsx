import { useEffect, useState } from "react";
import { CircleAlert, SquareTerminal } from "lucide-react";

import { plansFolder, planGuideText, TERM_DESKTOP_ONLY } from "../../../../src/core/terminal.ts";
import type { LeadItem } from "../../../../src/core/lead.ts";
import type { LeadState } from "../../data/useLead.ts";
import type { RunView } from "../../../../src/core/snapshot.ts";
import { errorText } from "../../lib/format.ts";
import type { Backend, TermSession } from "../../platform/backend/index.ts";
import { BUTTON_PRIMARY, BUTTON_SECONDARY } from "../../ui/controls.tsx";
import { PlanCard } from "../chat/LeadThread.tsx";
import { TerminalView } from "./TerminalView.tsx";

/**
 * The lead pane while its tab uses the Antigravity terminal (ADR-0042): a start panel
 * until `o term open` runs, then the terminal. The person talks there themselves — O
 * writes nothing — and plans they drop in the thread's plans folder appear below as
 * ordinary plan cards (`leadShow` keeps refreshing from App).
 */
export function LeadTerminal({
  api,
  project,
  thread,
  lead,
  session,
  sessionsReady,
  runs,
  onOpenCard,
  refresh,
}: {
  api: Backend;
  project: string;
  thread: string;
  lead: LeadState;
  session: TermSession | undefined;
  /** termList() answered at least once — before that the start panel must not flash. */
  sessionsReady: boolean;
  runs: RunView[];
  onOpenCard: (runId: string, cardId: string) => void;
  refresh: () => void;
}) {
  const [opening, setOpening] = useState(false);
  const [error, setError] = useState<string>();
  const [copied, setCopied] = useState(false);
  // The O state folder, for the plans-folder line the person pastes files into.
  const [home, setHome] = useState("");
  useEffect(() => {
    let live = true;
    void api.about().then((about) => {
      if (live) setHome(about.home);
    });
    return () => {
      live = false;
    };
  }, [api]);

  const open = async () => {
    setOpening(true);
    setError(undefined);
    try {
      await api.termOpen({ provider: "agy", project, thread });
      refresh();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setOpening(false);
    }
  };
  const plansDir = plansFolder(home || "%LOCALAPPDATA%\\project_O", thread);
  const copyGuide = async () => {
    try {
      await navigator.clipboard.writeText(planGuideText(plansDir));
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } catch {
      setCopied(false);
    }
  };
  // The dev server cannot open terminals (ADR-0042); the sentence explains it.
  const canDrive = api.canDriveTerminals;
  // Plans O picked up from the plans folder — the same cards the design view
  // draws — and the thread's errors (e.g. a rejected plan file), rendered as the
  // chat renders them so a failed file is not silent.
  const plans = lead.items.filter((item): item is Extract<LeadItem, { kind: "plan" }> => item.kind === "plan");
  const errors = lead.items.flatMap((item) => (item.kind === "error" ? [item] : []));

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex shrink-0 items-center gap-2 border-b px-6 py-2.5">
        <SquareTerminal className="size-4 shrink-0 text-chat-foreground-faint" aria-hidden="true" />
        <span className="min-w-0 truncate text-sm font-medium">Antigravity · 터미널</span>
        <button type="button" onClick={() => void copyGuide()} className={`ml-auto ${BUTTON_SECONDARY}`}>
          {copied ? "복사했습니다" : "계획 안내 복사"}
        </button>
      </header>
      {session ? (
        <>
          <TerminalView api={api} session={session} title={`Antigravity · ${thread}`} onClosed={refresh} />
          {session.state === "exited" && (
            // An exited session keeps its output visible; a new one is a button away.
            <div className="flex shrink-0 items-center justify-between gap-3 border-t px-6 py-2">
              <span className="text-xs text-muted-foreground">{canDrive ? "터미널이 종료됐습니다. 같은 대화에서 다시 시작할 수 있습니다." : TERM_DESKTOP_ONLY}</span>
              <button type="button" onClick={() => void open()} disabled={opening || !canDrive} title={canDrive ? undefined : TERM_DESKTOP_ONLY} className={BUTTON_SECONDARY}>
                {opening ? "여는 중…" : "새 터미널 열기"}
              </button>
            </div>
          )}
          {error && (
            <p role="alert" className="shrink-0 px-6 py-1.5 text-xs text-destructive">
              {error}
            </p>
          )}
        </>
      ) : sessionsReady ? (
        <div className="mx-auto flex max-w-md flex-1 flex-col items-center justify-center gap-3 px-6 text-center">
          <SquareTerminal className="size-8 text-chat-foreground-faint" aria-hidden="true" />
          <p className="text-sm text-chat-foreground">Antigravity는 공식 터미널 화면으로만 엽니다. O는 아무것도 입력하지 않습니다.</p>
          {!canDrive && <p className="text-xs text-muted-foreground">{TERM_DESKTOP_ONLY}</p>}
          <button type="button" onClick={() => void open()} disabled={opening || !canDrive} title={canDrive ? undefined : TERM_DESKTOP_ONLY} className={BUTTON_PRIMARY}>
            {opening ? "여는 중…" : "터미널 열기"}
          </button>
          {error && (
            <p role="alert" className="text-xs text-destructive">
              {error}
            </p>
          )}
          <p className="text-xs break-all text-chat-foreground-faint">
            계획 파일 폴더: <span className="select-all font-mono text-chat-code">{plansDir}</span>
          </p>
        </div>
      ) : (
        <div className="flex flex-1 items-center justify-center text-sm text-chat-foreground-faint">터미널을 찾는 중…</div>
      )}
      {(errors.length > 0 || plans.length > 0) && (
        <div className="scrollbar-sleek max-h-64 shrink-0 overflow-y-auto border-t">
          <div className="mx-auto flex max-w-3xl flex-col gap-2 px-6 py-4">
            {errors.map((item, i) => (
              <p key={`${item.at}-${i}`} role="alert" className="flex items-start gap-2 rounded-lg border border-destructive/40 px-3.5 py-2.5 text-sm">
                <CircleAlert className="mt-0.5 size-4 shrink-0 text-destructive" aria-hidden="true" />
                <span className="min-w-0 whitespace-pre-wrap">{item.text}</span>
              </p>
            ))}
            {plans.map((plan) => (
              <PlanCard key={plan.planId} plan={plan} lead={lead} run={runs.find((run) => run.runId === plan.runId)} onOpenCard={onOpenCard} />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
