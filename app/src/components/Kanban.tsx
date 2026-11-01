/*
 * Board primitives adapted from Kibo UI's Kanban (https://github.com/haydenbleasel/kibo,
 * packages/kanban/index.tsx at 3d63cdb). MIT License, Copyright (c) 2023 — Present shadcnblocks.
 * See THIRD_PARTY_NOTICES.md.
 *
 * Kibo's columns and cards, without its drag and drop: in O a column is a card's
 * observed state, so moving a card on screen must not pretend to change the work.
 */
import type { HTMLAttributes, ReactNode } from "react";

export interface KanbanColumn {
  id: string;
  name: string;
}

/**
 * Columns side by side when the board has room (a 56rem container); in a narrower
 * panel they stack as lanes so every column stays in view without sideways scrolling.
 */
export function KanbanProvider<C extends KanbanColumn>({ columns, children, className = "" }: { columns: C[]; children: (column: C) => ReactNode; className?: string }) {
  return (
    <div className="@container size-full">
      <div className={`grid content-start gap-3 @4xl:size-full @4xl:content-stretch @4xl:auto-cols-[minmax(12rem,1fr)] @4xl:grid-flow-col ${className}`}>{columns.map((column) => children(column))}</div>
    </div>
  );
}

export function KanbanBoard({ id, children, className = "" }: { id: string; children: ReactNode; className?: string }) {
  return (
    <section
      aria-labelledby={`kanban-${id}`}
      className={`flex flex-col divide-y overflow-hidden rounded-md border bg-secondary/60 text-xs shadow-sm @4xl:min-h-40 ${className}`}
    >
      {children}
    </section>
  );
}

export function KanbanHeader({ id, className = "", ...props }: HTMLAttributes<HTMLHeadingElement> & { id: string }) {
  return <h3 id={`kanban-${id}`} className={`m-0 flex items-center gap-2 p-2 text-sm font-semibold ${className}`} {...props} />;
}

export function KanbanCards({ children, className = "" }: { children: ReactNode; className?: string }) {
  return <div className={`scrollbar-sleek grid flex-grow grid-cols-[repeat(auto-fill,minmax(14rem,1fr))] content-start gap-2 overflow-y-auto p-2 @4xl:flex @4xl:flex-col ${className}`}>{children}</div>;
}

/** The whole card is one button; its text is what a screen reader hears, followed by the action. */
export function KanbanCard({ children, className = "", onOpen, action }: { children: ReactNode; className?: string; onOpen?: () => void; action: string }) {
  return (
    <button
      type="button"
      onClick={onOpen}
      className={`flex w-full flex-col gap-2.5 rounded-md border bg-card p-3 text-left text-card-foreground shadow-sm hover:bg-accent/30 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none ${className}`}
    >
      {children}
      <span className="sr-only">, {action}</span>
    </button>
  );
}
