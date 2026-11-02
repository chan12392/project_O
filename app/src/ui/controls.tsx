import type { ReactNode } from "react";

/**
 * Settings-style controls, after Orca's SettingsFormControls (row, switch,
 * segmented control, badge), shared by Settings, the agent setup and the account menu.
 */
export const BUTTON_PRIMARY =
  "inline-flex h-8 shrink-0 items-center justify-center gap-1.5 rounded-md bg-primary px-3 text-[13px] font-medium whitespace-nowrap text-primary-foreground hover:bg-primary/90 focus-ring disabled:cursor-not-allowed disabled:opacity-40";
export const BUTTON_SECONDARY =
  "inline-flex h-8 shrink-0 items-center justify-center gap-1.5 rounded-md border bg-background/40 px-3 text-[13px] whitespace-nowrap hover:bg-accent focus-ring disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:bg-transparent";
export const BUTTON_GHOST =
  "inline-flex h-8 shrink-0 items-center justify-center gap-1.5 rounded-md px-3 text-[13px] whitespace-nowrap text-muted-foreground hover:bg-accent hover:text-foreground focus-ring";

/** "준비 중": the label every not-yet-possible setting carries. */
export function SoonBadge() {
  return <span className="shrink-0 rounded-full bg-muted px-1.5 py-0.5 text-2xs font-medium whitespace-nowrap text-muted-foreground">준비 중</span>;
}

export function SettingsRow({ label, description, control, id, soon }: { label: ReactNode; description?: ReactNode; control: ReactNode; id?: string; soon?: boolean }) {
  return (
    <div className="flex items-center justify-between gap-6 py-3.5">
      <div className="min-w-0 flex-1 space-y-1">
        <p id={id} className="flex flex-wrap items-center gap-2 text-[13px] font-medium">
          {label}
          {soon && <SoonBadge />}
        </p>
        {description && <div className="text-xs leading-relaxed text-muted-foreground">{description}</div>}
      </div>
      <div className="shrink-0">{control}</div>
    </div>
  );
}

export function Switch({ checked, onChange, labelledBy, disabled }: { checked: boolean; onChange: (next: boolean) => void; labelledBy: string; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-labelledby={labelledBy}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className="group relative inline-flex h-[18px] w-8 shrink-0 items-center rounded-full border bg-muted transition-colors duration-150 focus-ring aria-checked:border-transparent aria-checked:bg-primary disabled:cursor-not-allowed disabled:opacity-50"
    >
      <span className="block size-3 translate-x-[2px] rounded-full bg-muted-foreground shadow-sm transition-transform duration-150 group-aria-checked:translate-x-[16px] group-aria-checked:bg-primary-foreground" />
    </button>
  );
}

export interface Option<T extends string> {
  value: T;
  label: string;
  /** Shown but not choosable yet ("준비 중"). */
  soon?: boolean;
}

/** Orca's segmented control: a radio group of small buttons; options not ready stay visible but inert. */
/** `disabled` greys the whole group out while the setting it picks has no effect; the choice stays shown. */
export function Segmented<T extends string>({ value, options, onChange, label, disabled }: { value: T; options: Option<T>[]; onChange?: (value: T) => void; label: string; disabled?: boolean }) {
  return (
    <div role="radiogroup" aria-label={label} aria-disabled={disabled || undefined} className={`inline-flex items-center rounded-md border bg-background/50 p-0.5 ${disabled ? "opacity-50" : ""}`}>
      {options.map((o) => {
        const active = o.value === value;
        const inert = o.soon || !onChange || disabled;
        return (
          <button
            key={o.value}
            type="button"
            role="radio"
            aria-checked={active}
            aria-disabled={inert && (!active || disabled) ? true : undefined}
            title={o.soon ? "준비 중" : undefined}
            onClick={() => !inert && onChange?.(o.value)}
            className={`rounded-sm px-3 py-1 text-xs whitespace-nowrap transition-colors focus-ring ${
              active ? "bg-accent font-medium text-accent-foreground" : inert ? "cursor-not-allowed text-muted-foreground/50" : "text-muted-foreground hover:text-foreground"
            }`}
          >
            {o.label}
          </button>
        );
      })}
    </div>
  );
}

/** A section's own sub-heading inside a settings page. */
export function SubHeading({ title, description }: { title: string; description?: ReactNode }) {
  return (
    <div className="space-y-1 pb-1">
      <h3 className="text-sm font-semibold">{title}</h3>
      {description && <p className="text-xs leading-relaxed text-muted-foreground">{description}</p>}
    </div>
  );
}
