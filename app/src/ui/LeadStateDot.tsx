import { LEAD_TAB_WORD, type LeadTab } from "../../../src/core/lead.ts";

/**
 * A lead conversation's state dot (ADR-0034), shared by the header tabs and the sidebar:
 * idle gray, waiting orange (확인 필요 — the person's action is needed), working green with a
 * ring. The ring's keyframes live in orca-theme.css (the CSP forbids injected styles) and the
 * motion setting holds them.
 */
export function LeadStateDot({ state }: { state: LeadTab["state"] }) {
  const label = LEAD_TAB_WORD[state];
  const dot = state === "working" ? "bg-agent-working" : state === "waiting" ? "bg-status-warning" : "bg-muted-foreground/50";
  return (
    <span role="img" aria-label={label} title={label} className="relative inline-flex size-2 shrink-0 items-center justify-center">
      <span className={`block size-2 rounded-full ${dot}`} />
      {state === "working" && <span aria-hidden="true" className="lead-dot-ring" />}
    </span>
  );
}
