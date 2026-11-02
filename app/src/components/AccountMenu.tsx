import { ChevronsUpDown, CircleHelp, LogIn, Plug, RefreshCw, Settings as SettingsIcon, UserRound } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";

import { useDismiss } from "../dismiss.ts";
import { ICON_BUTTON } from "../format.ts";

export const ACCOUNT_BUTTON_ID = "account-menu-button";
export const SETTINGS_BUTTON_ID = "sidebar-settings-button";

function Avatar({ size = "size-7" }: { size?: string }) {
  return (
    <span className={`flex ${size} shrink-0 items-center justify-center rounded-full bg-muted text-muted-foreground`} aria-hidden="true">
      <UserRound className="size-4" />
    </span>
  );
}

/**
 * The sidebar footer, after Orca's: the local profile (sign-in comes later) with
 * a small menu above it, and a settings button.
 */
export function AccountMenu({ onSettings, onAbout, onAgents, onRefreshUsage }: { onSettings: () => void; onAbout: () => void; onAgents: () => void; onRefreshUsage?: () => void }) {
  const [open, setOpen] = useState(false);
  const panel = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);
  useDismiss(panel, open, close, button);
  useEffect(() => {
    if (open) panel.current?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
  }, [open]);
  const choose = (action: () => void) => () => {
    close();
    action();
  };
  const item = "flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left text-[13px] hover:bg-accent focus-ring disabled:cursor-not-allowed disabled:text-muted-foreground disabled:hover:bg-transparent";
  const icon = "size-4 shrink-0 text-muted-foreground";

  return (
    <div className="relative flex shrink-0 items-center gap-1 border-t px-2 py-2">
      <button
        ref={button}
        id={ACCOUNT_BUTTON_ID}
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        aria-haspopup="true"
        aria-label="계정 메뉴, 로그인하지 않음"
        className="flex min-w-0 flex-1 items-center gap-2.5 rounded-md px-1.5 py-1 text-left hover:bg-sidebar-accent aria-expanded:bg-sidebar-accent focus-ring"
      >
        <Avatar />
        <span className="min-w-0 flex-1 truncate text-[13px] text-sidebar-foreground/90">로그인하지 않음</span>
        <ChevronsUpDown className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
      </button>
      <button id={SETTINGS_BUTTON_ID} type="button" onClick={onSettings} aria-label="설정" title="설정" className={ICON_BUTTON}>
        <SettingsIcon className="size-4" aria-hidden="true" />
      </button>

      {open && (
        <div ref={panel} role="group" aria-label="계정 메뉴" className="bloom absolute right-2 bottom-full left-2 z-30 mb-1 rounded-lg border bg-popover p-1 text-popover-foreground shadow-lg">
          <div className="flex items-center gap-2.5 px-2 pt-1.5 pb-2">
            <Avatar size="size-8" />
            <div className="min-w-0">
              <p className="truncate text-[13px] font-medium">로그인하지 않음</p>
              <p className="truncate text-2xs text-muted-foreground">이 PC에만 있는 로컬 프로필</p>
            </div>
          </div>
          <div className="my-1 border-t" />
          <button type="button" className={item} onClick={choose(onSettings)}>
            <SettingsIcon className={icon} aria-hidden="true" />
            설정
          </button>
          <button type="button" className={item} onClick={choose(onAgents)}>
            <Plug className={icon} aria-hidden="true" />
            에이전트 연결
          </button>
          {onRefreshUsage && (
            <button type="button" className={item} onClick={choose(onRefreshUsage)}>
              <RefreshCw className={icon} aria-hidden="true" />
              사용량 새로 고침
            </button>
          )}
          <button type="button" className={item} onClick={choose(onAbout)}>
            <CircleHelp className={icon} aria-hidden="true" />
            도움말·정보
          </button>
          <div className="my-1 border-t" />
          <button type="button" className={item} disabled>
            <LogIn className={icon} aria-hidden="true" />
            <span className="min-w-0 flex-1">로그인</span>
            <span className="text-2xs whitespace-nowrap text-muted-foreground">Google·카카오·Apple 준비 중</span>
          </button>
        </div>
      )}
    </div>
  );
}
