import { X } from "lucide-react";
import { useEffect, useRef, useState } from "react";

/** Raised by the desktop shell when a link was not opened (app/src-tauri/src/commands/window.rs). */
const LINK_BLOCKED = "project-o:link-blocked";
const SHOW_MS = 10_000;

interface Blocked {
  url: string;
  message: string;
}

/**
 * Why a link did nothing: the desktop app opens only https pages, in the person's browser.
 * Mounted once per window (main.tsx); quiet everywhere else.
 */
export function LinkNotice() {
  const [blocked, setBlocked] = useState<Blocked>();
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const show = (event: Event) => setBlocked((event as CustomEvent<Blocked>).detail);
    window.addEventListener(LINK_BLOCKED, show);
    return () => window.removeEventListener(LINK_BLOCKED, show);
  }, []);

  useEffect(() => {
    if (!blocked) return;
    // Stays while the person is on its close button.
    const timer = window.setTimeout(() => {
      if (!box.current?.contains(document.activeElement)) setBlocked(undefined);
    }, SHOW_MS);
    return () => window.clearTimeout(timer);
  }, [blocked]);

  if (!blocked) return null;
  return (
    <div
      ref={box}
      role="alert"
      className="bloom fixed inset-x-4 bottom-12 z-50 mx-auto flex max-w-lg items-start gap-3 rounded-lg border bg-background p-3 text-[13px] text-foreground shadow-lg"
    >
      <div className="min-w-0 flex-1">
        <p>{blocked.message}</p>
        <p className="mt-1 truncate font-mono text-xs text-muted-foreground" title={blocked.url}>
          {blocked.url}
        </p>
      </div>
      <button
        type="button"
        aria-label="안내 닫기"
        onClick={() => setBlocked(undefined)}
        className="-m-1 inline-flex size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground focus-ring"
      >
        <X className="size-4" aria-hidden="true" />
      </button>
    </div>
  );
}
