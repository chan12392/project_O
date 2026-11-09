import { FilePen, FileText, Globe, LoaderCircle, Search, ShieldQuestion, SquareTerminal, Wrench } from "lucide-react";

import type { LeadStep, LeadWorkEntry } from "../../../../src/core/lead.ts";
import { decisionText, lineCounts, stepLine, workSummary, approvalKind } from "../../../../src/core/leadWork.ts";

const ICONS = { read: FileText, search: Search, run: SquareTerminal, edit: FilePen, web: Globe, other: Wrench } as const;

/** One step: its kind's icon (a spinner while it runs), what it does, its target and an edit's counts. */
function StepRow({ step }: { step: LeadStep }) {
  const line = stepLine(step);
  const Icon = line.running ? LoaderCircle : ICONS[step.kind as keyof typeof ICONS] ?? Wrench;
  const counts = lineCounts(line.added, line.removed);
  return (
    <li className="flex min-w-0 items-center gap-1.5" title={step.file ?? step.label}>
      <Icon className={`size-3 shrink-0 ${line.running ? "animate-spin motion-reduce:animate-none" : ""}`} aria-hidden="true" />
      {step.kind === "edit" && !line.running ? (
        <span className="min-w-0 truncate font-mono text-chat-foreground">'{line.target}'</span>
      ) : (
        <>
          <span className="shrink-0">{line.verb}</span>
          <span className="min-w-0 truncate font-mono">{line.target}</span>
        </>
      )}
      {counts && (
        <span className="shrink-0 font-mono tabular-nums">
          <span className="text-status-success">+{line.added}</span> <span className="text-destructive">−{line.removed}</span>
        </span>
      )}
      {line.note && <span className="shrink-0 text-status-warning-text">· {line.note}</span>}
    </li>
  );
}

function ApprovalRow({ entry }: { entry: Extract<LeadWorkEntry, { type: "approval" }> }) {
  const refused = entry.decision !== undefined && entry.decision !== "allow";
  return (
    <li className="flex min-w-0 items-center gap-1.5" title={entry.reason ?? entry.summary}>
      <ShieldQuestion className="size-3 shrink-0" aria-hidden="true" />
      <span className="shrink-0">{approvalKind(entry.kind)} 승인</span>
      <span className="min-w-0 truncate font-mono">{entry.summary}</span>
      <span className={`shrink-0 ${refused ? "text-status-warning-text" : ""}`}>· {decisionText(entry.decision, entry.by)}</span>
    </li>
  );
}

function Entries({ entries }: { entries: readonly LeadWorkEntry[] }) {
  return (
    <ul className="space-y-1">
      {entries.map((entry, i) => entry.type === "step"
        ? <StepRow key={`s-${entry.step.id}-${i}`} step={entry.step} />
        : <ApprovalRow key={`a-${entry.id}`} entry={entry} />)}
    </ul>
  );
}

/** While the lead answers: every step so far, the latest at the bottom, kept to the last few. */
export function LiveWork({ steps, waiting }: { steps: readonly LeadStep[]; waiting: number }) {
  const shown = steps.slice(-8);
  const hidden = steps.length - shown.length;
  return (
    <div className="space-y-1">
      {hidden > 0 && <p>앞선 {hidden}단계</p>}
      <Entries entries={shown.map((step) => ({ type: "step", step }))} />
      {waiting > 0 && (
        <p className="flex items-center gap-1.5 text-status-warning-text">
          <ShieldQuestion className="size-3 shrink-0" aria-hidden="true" />
          승인을 기다리는 작업 {waiting}개 · 입력창 위에서 허용하거나 거부하세요
        </p>
      )}
    </div>
  );
}

/** After the turn: its steps and approvals folded above the answer ("작업 n단계"). */
export function WorkList({ entries }: { entries: readonly LeadWorkEntry[] }) {
  if (entries.length === 0) return null;
  return (
    <details className="group text-xs text-chat-foreground-faint">
      <summary className="flex w-fit cursor-pointer items-center gap-1.5 rounded py-0.5 hover:text-chat-foreground focus-ring">
        <Wrench className="size-3 shrink-0" aria-hidden="true" />
        {workSummary(entries)}
      </summary>
      <div className="mt-1.5 border-l pl-3">
        <Entries entries={entries} />
      </div>
    </details>
  );
}
