import type { GraphColorBy, GraphView } from "../../platform/backend/types.ts";
import { legendItems, type GraphTheme } from "./graphColors.ts";

/**
 * The color key for what the canvas shows, floating over its bottom-right corner —
 * only the categories actually on screen (ADR-0033). `--muted-foreground` is the same
 * neutral the canvas falls back to for uncolored files.
 */
export function GraphLegend({ view, colorBy, dark }: { view: Pick<GraphView, "nodes">; colorBy: GraphColorBy; dark: boolean }) {
  const theme: GraphTheme = dark ? "dark" : "light";
  const items = legendItems(view, colorBy, theme, "var(--muted-foreground)");
  if (items.length === 0) return null;
  return (
    <ul aria-label="색 범례" className="absolute bottom-2 right-2 z-10 flex max-w-44 flex-col gap-1 rounded-md bg-card/80 px-2 py-1.5 text-2xs text-muted-foreground shadow">
      {items.map((item) => (
        <li key={item.label} className="flex items-center gap-1.5">
          <span aria-hidden="true" className="size-2 shrink-0 rounded-full" style={{ backgroundColor: item.color }} />
          <span className="truncate">{item.label}</span>
        </li>
      ))}
    </ul>
  );
}
