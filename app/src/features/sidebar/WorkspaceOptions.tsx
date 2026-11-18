import { SlidersHorizontal } from "lucide-react";
import { useCallback, useId, useRef, useState, type KeyboardEvent } from "react";

import { GROUP_BYS, RUN_LIMITS, SORT_BYS, type GroupBy, type SidebarView, type SortBy } from "../../data/sidebarView.ts";
import { useDismiss } from "../../lib/dismiss.ts";
import { Switch } from "../../ui/controls.tsx";

const GROUP_WORD: Record<GroupBy, string> = { none: "없음", group: "사용자 그룹", status: "상태" };
const SORT_WORD: Record<SortBy, string> = { recent: "최근 활동", name: "이름", manual: "수동" };

const GROUP_OPTIONS = GROUP_BYS.map((value) => ({ value, label: GROUP_WORD[value] }));
const SORT_OPTIONS = SORT_BYS.map((value) => ({ value, label: SORT_WORD[value] }));
const LIMIT_OPTIONS = RUN_LIMITS.map((value) => ({ value, label: value === "all" ? "전체" : String(value) }));

const HEADING = "px-2 pt-1 pb-1.5 text-3xs font-semibold text-muted-foreground";

/**
 * The sidebar's 워크스페이스 옵션: a button and the panel it opens. They come back side by side,
 * not wrapped: the panel hangs under the title row (its nearest positioned ancestor), so it spans
 * the sidebar wherever the button sits. The panel stays open while options change.
 */
export function WorkspaceOptions({ view, onChange, buttonClass }: {
  view: SidebarView;
  onChange: (change: Partial<SidebarView>) => void;
  /** The title row's icon-button look (Sidebar's HEAD_BUTTON), plus any margin. */
  buttonClass: string;
}) {
  const [open, setOpen] = useState(false);
  const panel = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);
  useDismiss(panel, open, close, button);
  const id = useId();
  const manual = view.sortBy === "manual";
  return (
    <>
      <button ref={button} type="button" onClick={() => setOpen((o) => !o)} aria-expanded={open} aria-label="워크스페이스 옵션" title="워크스페이스 옵션" className={buttonClass}>
        <SlidersHorizontal className="size-3.5" aria-hidden="true" />
      </button>
      {open && (
        <div ref={panel} role="group" aria-label="워크스페이스 옵션" className="bloom absolute top-full right-2 left-2 z-20 mt-1 rounded-lg border bg-popover p-1 text-popover-foreground shadow-lg">
          <p id={`${id}-group`} className={HEADING}>그룹화 기준</p>
          <div className="px-2 pb-1.5">
            <Choice value={view.groupBy} options={GROUP_OPTIONS} onChange={(groupBy) => onChange({ groupBy })} labelledBy={`${id}-group`} />
          </div>

          <p id={`${id}-sort`} className={HEADING}>정렬 기준</p>
          <div className="px-2 pb-1.5">
            <Choice value={view.sortBy} options={SORT_OPTIONS} onChange={(sortBy) => onChange({ sortBy })} labelledBy={`${id}-sort`} describedBy={manual ? `${id}-manual` : undefined} />
            {manual && (
              <p id={`${id}-manual`} className="pt-1.5 text-3xs leading-relaxed text-muted-foreground">
                프로젝트 줄을 끌어 순서를 바꿉니다. 키보드로는 프로젝트의 ⋯ 메뉴에서 위로·아래로 옮깁니다.
              </p>
            )}
          </div>

          <p className={HEADING}>필터</p>
          <div className="flex items-center justify-between gap-2 px-2 pb-1.5">
            <span id={`${id}-scheduled`} className="min-w-0 text-xs">자동화로 만든 실행 숨기기</span>
            <Switch checked={view.hideScheduled} onChange={(hideScheduled) => onChange({ hideScheduled })} labelledBy={`${id}-scheduled`} />
          </div>

          <p id={`${id}-limit`} className={HEADING}>실행 표시 (프로젝트당 줄 수)</p>
          <div className="px-2 pb-1">
            <Choice value={view.runLimit} options={LIMIT_OPTIONS} onChange={(runLimit) => onChange({ runLimit })} labelledBy={`${id}-limit`} />
          </div>
        </div>
      )}
    </>
  );
}

/**
 * A full-width segmented radio group. One Tab stop (the checked option); the arrow keys, Home and
 * End move to an option and choose it, as a native radio group does.
 */
function Choice<T extends string | number>({ value, options, onChange, labelledBy, describedBy }: {
  value: T;
  options: readonly { value: T; label: string }[];
  onChange: (value: T) => void;
  labelledBy: string;
  describedBy?: string;
}) {
  const group = useRef<HTMLDivElement>(null);
  const checked = options.findIndex((o) => o.value === value);
  const choose = (index: number) => {
    const at = (index + options.length) % options.length;
    const option = options[at];
    if (!option) return;
    if (option.value !== value) onChange(option.value);
    group.current?.querySelectorAll<HTMLElement>('[role="radio"]')[at]?.focus();
  };
  const onKeyDown = (e: KeyboardEvent, index: number) => {
    const to =
      e.key === "ArrowLeft" || e.key === "ArrowUp" ? index - 1
      : e.key === "ArrowRight" || e.key === "ArrowDown" ? index + 1
      : e.key === "Home" ? 0
      : e.key === "End" ? options.length - 1
      : undefined;
    if (to === undefined) return;
    e.preventDefault();
    choose(to);
  };
  return (
    <div ref={group} role="radiogroup" aria-labelledby={labelledBy} aria-describedby={describedBy} className="flex w-full items-center rounded-md border bg-background/50 p-0.5">
      {options.map((o, i) => {
        const active = i === checked;
        return (
          <button
            key={o.value}
            type="button"
            role="radio"
            aria-checked={active}
            // A value no option has would leave the group without a Tab stop: the first one takes it.
            tabIndex={active || (checked < 0 && i === 0) ? 0 : -1}
            onClick={() => choose(i)}
            onKeyDown={(e) => onKeyDown(e, i)}
            className={`flex-1 rounded-sm px-1.5 py-1 text-xs whitespace-nowrap transition-colors focus-ring ${active ? "bg-accent font-medium text-accent-foreground" : "text-muted-foreground hover:text-foreground"}`}
          >
            {o.label}
          </button>
        );
      })}
    </div>
  );
}
