import { ChevronDown } from "lucide-react";
import { useCallback, useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode, type RefObject } from "react";

import { useDismiss } from "../lib/dismiss.ts";

export interface MenuChoice {
  value: string;
  label: string;
  /** A second, quieter line. */
  hint?: string;
  /** A short id on the same line, at its end (a model alias); keeps the row one line tall. */
  aside?: string;
  /** The row's tooltip. */
  title?: string;
  disabled?: boolean;
}

export interface MenuGroup {
  label: string;
  value: string;
  choices: MenuChoice[];
  onChoose: (value: string) => void;
  /**
   * This group's choices scroll inside the menu (its label stays put) and give up their
   * height first, so a long list never makes the menu taller and the other groups stay in view.
   */
  scroll?: boolean;
}

/**
 * A small upward menu of radio choices in groups (the composer's agent·model·effort and
 * run profile pickers). Arrow keys move between choices, Home/End jump, Escape or a press
 * outside closes it and focus returns to the trigger. Choosing keeps it open only
 * when `stayOpen` says so (picking an agent, then its model and effort). The menu is never
 * taller than `MENU_MAX` (and the window): a `scroll` group scrolls inside it with a visible
 * scrollbar, and moving focus keeps the focused row in view.
 */
/** The menu's height cap: fits a 700px-tall window above the composer, and stays the same in a taller one. */
const MENU_MAX = "max-h-[min(36rem,calc(100vh-8rem))]";

/** Focus a row and bring it into view within its own scrolling list, without moving anything else more than needed. */
function focusRow(row: HTMLButtonElement | undefined) {
  if (!row) return;
  row.focus({ preventScroll: true });
  row.scrollIntoView({ block: "nearest" });
}

export function ChoiceMenu({
  label,
  trigger,
  groups,
  triggerClass,
  align = "left",
  stayOpen,
  disabled,
  triggerRef,
  title,
}: {
  label: string;
  /** The trigger's tooltip, when its short text needs saying in full. */
  title?: string;
  trigger: ReactNode;
  groups: MenuGroup[];
  triggerClass: string;
  align?: "left" | "right";
  stayOpen?: (group: string, value: string) => boolean;
  disabled?: boolean;
  /** The trigger, for a caller that moves focus back to it later (after a follow-up field). */
  triggerRef?: RefObject<HTMLButtonElement | null>;
}) {
  const [open, setOpen] = useState(false);
  const panel = useRef<HTMLDivElement>(null);
  const own = useRef<HTMLButtonElement>(null);
  const button = triggerRef ?? own;
  const id = useId();
  const close = useCallback(() => setOpen(false), []);
  useDismiss(panel, open, close, button);

  const items = () => [...(panel.current?.querySelectorAll<HTMLButtonElement>("[role=menuitemradio]:not(:disabled)") ?? [])];
  useEffect(() => {
    if (!open) return;
    const all = items();
    focusRow(all.find((b) => b.getAttribute("aria-checked") === "true") ?? all[0]);
  }, [open]);

  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    const all = items();
    const at = all.indexOf(document.activeElement as HTMLButtonElement);
    const move = { ArrowDown: at + 1, ArrowUp: at - 1, Home: 0, End: all.length - 1 }[e.key];
    if (move === undefined) {
      if (e.key === "Tab") close();
      return;
    }
    e.preventDefault();
    focusRow(all[(move + all.length) % all.length]);
  };

  return (
    <div className="relative">
      <button
        ref={button}
        type="button"
        disabled={disabled}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        aria-label={label}
        title={title}
        onClick={() => setOpen((o) => !o)}
        onKeyDown={(e) => {
          if (e.key === "ArrowUp" || e.key === "ArrowDown") {
            e.preventDefault();
            setOpen(true);
          }
        }}
        className={triggerClass}
      >
        {trigger}
        <ChevronDown className="size-3.5 shrink-0 opacity-70" aria-hidden="true" />
      </button>
      {open && (
        <div
          ref={panel}
          id={id}
          role="menu"
          aria-label={label}
          onKeyDown={onKey}
          className={`bloom scrollbar-sleek absolute bottom-full z-30 mb-2 flex ${MENU_MAX} w-64 flex-col overflow-y-auto overscroll-contain rounded-lg border bg-popover p-1 text-popover-foreground shadow-lg ${align === "right" ? "right-0" : "left-0"}`}
        >
          {groups.map((group, i) => (
            <div
              key={group.label}
              role="group"
              aria-label={group.label}
              className={`${i > 0 ? "mt-1 border-t pt-1" : ""} ${group.scroll ? "flex min-h-24 shrink flex-col" : "shrink-0"}`}
            >
              <p className="shrink-0 px-2 pt-1 pb-0.5 text-2xs text-muted-foreground" aria-hidden="true">
                {group.label}
              </p>
              <div className={group.scroll ? "scrollbar-sleek min-h-0 overflow-y-auto overscroll-contain [scrollbar-gutter:stable]" : undefined}>
                {group.choices.map((choice) => {
                  const checked = choice.value === group.value;
                  return (
                    <button
                      key={choice.value}
                      type="button"
                      role="menuitemradio"
                      aria-checked={checked}
                      disabled={choice.disabled}
                      title={choice.title}
                      onClick={() => {
                        group.onChoose(choice.value);
                        if (!stayOpen?.(group.label, choice.value)) {
                          close();
                          button.current?.focus();
                        }
                      }}
                      className="flex w-full items-start gap-2 rounded-md px-2 py-1.5 text-left text-[13px] hover:bg-accent focus-ring focus-visible:ring-inset aria-checked:bg-accent/60 disabled:cursor-not-allowed disabled:text-muted-foreground disabled:hover:bg-transparent"
                    >
                      <span className={`mt-1.5 size-1.5 shrink-0 rounded-full ${checked ? "bg-primary" : "bg-transparent"}`} aria-hidden="true" />
                      <span className="min-w-0 flex-1">
                        <span className="flex items-baseline gap-2">
                          <span className="min-w-0">{choice.label}</span>
                          {choice.aside && <span className="ml-auto shrink-0 font-mono text-2xs text-muted-foreground">{choice.aside}</span>}
                        </span>
                        {choice.hint && <span className="block text-2xs leading-snug text-muted-foreground">{choice.hint}</span>}
                      </span>
                    </button>
                  );
                })}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
