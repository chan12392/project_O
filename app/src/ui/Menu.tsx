import { ChevronDown } from "lucide-react";
import { useCallback, useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";

import { useDismiss } from "../lib/dismiss.ts";

export interface MenuChoice {
  value: string;
  label: string;
  /** A second, quieter line. */
  hint?: string;
  disabled?: boolean;
}

export interface MenuGroup {
  label: string;
  value: string;
  choices: MenuChoice[];
  onChoose: (value: string) => void;
}

/**
 * A small upward menu of radio choices in groups (the composer's agent·effort and run
 * profile pickers). Arrow keys move between choices, Home/End jump, Escape or a press
 * outside closes it and focus returns to the trigger. Choosing keeps it open only
 * when `stayOpen` says so (picking an agent, then its effort).
 */
export function ChoiceMenu({
  label,
  trigger,
  groups,
  triggerClass,
  align = "left",
  stayOpen,
  disabled,
}: {
  label: string;
  trigger: ReactNode;
  groups: MenuGroup[];
  triggerClass: string;
  align?: "left" | "right";
  stayOpen?: (group: string) => boolean;
  disabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const panel = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const id = useId();
  const close = useCallback(() => setOpen(false), []);
  useDismiss(panel, open, close, button);

  const items = () => [...(panel.current?.querySelectorAll<HTMLButtonElement>("[role=menuitemradio]:not(:disabled)") ?? [])];
  useEffect(() => {
    if (!open) return;
    const all = items();
    (all.find((b) => b.getAttribute("aria-checked") === "true") ?? all[0])?.focus();
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
    all[(move + all.length) % all.length]?.focus();
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
          className={`bloom absolute bottom-full z-30 mb-2 w-64 rounded-lg border bg-popover p-1 text-popover-foreground shadow-lg ${align === "right" ? "right-0" : "left-0"}`}
        >
          {groups.map((group, i) => (
            <div key={group.label} role="group" aria-label={group.label} className={i > 0 ? "mt-1 border-t pt-1" : undefined}>
              <p className="px-2 pt-1 pb-0.5 text-2xs text-muted-foreground" aria-hidden="true">
                {group.label}
              </p>
              {group.choices.map((choice) => {
                const checked = choice.value === group.value;
                return (
                  <button
                    key={choice.value}
                    type="button"
                    role="menuitemradio"
                    aria-checked={checked}
                    disabled={choice.disabled}
                    onClick={() => {
                      group.onChoose(choice.value);
                      if (!stayOpen?.(group.label)) {
                        close();
                        button.current?.focus();
                      }
                    }}
                    className="flex w-full items-start gap-2 rounded-md px-2 py-1.5 text-left text-[13px] hover:bg-accent focus-ring focus-visible:ring-inset aria-checked:bg-accent/60 disabled:cursor-not-allowed disabled:text-muted-foreground disabled:hover:bg-transparent"
                  >
                    <span className={`mt-1.5 size-1.5 shrink-0 rounded-full ${checked ? "bg-primary" : "bg-transparent"}`} aria-hidden="true" />
                    <span className="min-w-0 flex-1">
                      <span className="block">{choice.label}</span>
                      {choice.hint && <span className="block text-2xs leading-snug text-muted-foreground">{choice.hint}</span>}
                    </span>
                  </button>
                );
              })}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
