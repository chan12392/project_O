/**
 * Backend answers as safe display values (ADR-0033). Labels and summaries can carry
 * Deep's LLM output, and counts or the Deep stamp can arrive missing or NaN — every
 * helper here is pure so it is testable without a browser.
 */
import type { GraphDeepStamp } from "../../platform/backend/types.ts";

const ESCAPE: Record<string, string> = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };

/**
 * A plain string made safe for HTML context. Needed because 3d-force-graph's
 * `nodeLabel` tooltip writes the returned string with `innerHTML`, so a label like
 * `<img onerror=…>` would otherwise run as markup.
 */
export function escapeHtml(text: string): string {
  return text.replace(/[&<>"']/g, (c) => ESCAPE[c] ?? c);
}

/** The "Deep: provider model · date" stamp; an unparseable `at` just drops the date. */
export function deepStampText(deep: GraphDeepStamp): string {
  const stamp = `Deep: ${deep.provider} ${deep.model}`;
  const at = Date.parse(deep.at);
  return Number.isNaN(at) ? stamp : `${stamp} · ${new Date(at).toLocaleDateString()}`;
}

/** A count for display; a non-finite backend answer shows as "—" instead of "NaN". */
export function countText(n: number): string {
  return Number.isFinite(n) ? String(n) : "—";
}

/** Type guard: a backend number only when it is a real finite value (not null, NaN or ∞). */
export function finite(n: number | null | undefined): n is number {
  return typeof n === "number" && Number.isFinite(n);
}
