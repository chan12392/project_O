import { useEffect, useRef, useState } from "react";

import { b64ToBytes, bytesToB64, nextOffset, TERM_DESKTOP_ONLY, termWriteChunks, termWriteQueueEnd, termWriteQueueFor, type TermSession, type TermWriteQueue } from "../../../../src/core/terminal.ts";
import { errorText } from "../../lib/format.ts";
import type { Backend } from "../../platform/backend/types.ts";
import { BUTTON_GHOST, BUTTON_SECONDARY } from "../../ui/controls.tsx";
import { loadXterm } from "./xtermLib.ts";

const UTF8 = new TextEncoder();

const STATE_WORD: Record<string, string> = { running: "실행 중", "awaiting-paste": "붙여넣기 대기", exited: "종료됨" };

/**
 * One terminal session shown as it is (ADR-0042): raw output bytes go to xterm
 * and keystrokes go back through `termWrite`; the screen never re-reads them as
 * chat. Output is polled by offset (~80 ms while running, slower when hidden,
 * stopped when exited), and bytes the host dropped — a risen `start` — reset the
 * screen. xterm and its stylesheet load lazily like the Graph tab's `three`.
 */
export function TerminalView({ api, session, title, onClosed, className }: { api: Backend; session: TermSession; title?: string; onClosed?: () => void; className?: string }) {
  const host = useRef<HTMLDivElement>(null);
  const [state, setState] = useState(session.state);
  const [exitCode, setExitCode] = useState<number | null>(session.exitCode);
  const [failed, setFailed] = useState("");
  const [retrying, setRetrying] = useState(false);
  const [inputBroken, setInputBroken] = useState(false);
  const [busy, setBusy] = useState(false);
  const queueRef = useRef<TermWriteQueue | undefined>(undefined);
  // The dev server cannot run the human-only operations (ADR-0042): the terminal
  // is then read-only — output and resize still work.
  const canDrive = api.canDriveTerminals;

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    let live = true;
    let offset = 0;
    let readFails = 0;
    let timer = 0;
    let term: import("@xterm/xterm").Terminal | undefined;
    const cleanup: (() => void)[] = [];

    /** The app's tokens as xterm colors: this element's resolved canvas and ink. */
    const themeOf = () => {
      const cs = getComputedStyle(el);
      return { background: cs.backgroundColor, foreground: cs.color, cursor: cs.color, cursorAccent: cs.backgroundColor, selectionBackground: "rgba(128,128,128,0.35)" };
    };

    const poll = async () => {
      try {
        const chunk = await api.termRead(session.id, offset);
        if (!live) return;
        readFails = 0;
        setRetrying(false);
        const next = nextOffset(offset, chunk);
        if (next.stale) term?.reset();
        offset = next.offset;
        if (chunk.data) term?.write(b64ToBytes(chunk.data));
        setState(chunk.state);
        setExitCode(chunk.exitCode);
        if (chunk.state === "exited") {
          // The host may still hold trailing bytes past this chunk: one last read
          // at the final offset so the end of the output is not lost.
          try {
            const tail = await api.termRead(session.id, offset);
            if (live && tail.data) term?.write(b64ToBytes(tail.data));
          } catch {
            // Trailing output is best-effort; the session is already over.
          }
          termWriteQueueEnd(session.id);
          return;
        }
      } catch {
        // A transient read error (e.g. the output file is being trimmed) is not
        // an exit: keep polling with backoff and say so once it keeps failing.
        if (!live) return;
        readFails++;
        if (readFails >= 3) setRetrying(true);
      }
      if (live) timer = window.setTimeout(() => void poll(), readFails ? Math.min(2_000, 250 * 2 ** readFails) : document.hidden ? 1_000 : 80);
    };

    void loadXterm().then(({ Terminal, FitAddon }) => {
      if (!live) return;
      term = new Terminal({ allowTransparency: true, cursorBlink: true, fontFamily: "ui-monospace, 'Cascadia Mono', Consolas, monospace", fontSize: 13, screenReaderMode: true, theme: themeOf() });
      const fit = new FitAddon();
      term.loadAddon(fit);
      term.open(el);
      fit.fit();
      void api.termResize(session.id, term.cols, term.rows).catch(() => {});
      // Keystrokes serialize through the session's one FIFO (typing order, shared
      // by every view of the session) in ≤32 KiB pieces that never split a
      // character; a failure breaks the queue until the person retries.
      const queue = termWriteQueueFor(session.id, (b64) => api.termWrite(session.id, b64));
      queueRef.current = queue;
      term.onData((data) => {
        if (!canDrive) return;
        for (const piece of termWriteChunks(UTF8.encode(data))) void queue.send(bytesToB64(piece)).catch(() => setInputBroken(true));
      });
      const sizes = new ResizeObserver(() => {
        fit.fit();
        if (term) void api.termResize(session.id, term.cols, term.rows).catch(() => {});
      });
      sizes.observe(el);
      // Follow the app's light/dark switch: the html class carries it.
      const dark = new MutationObserver(() => {
        if (term) term.options.theme = themeOf();
      });
      dark.observe(document.documentElement, { attributes: true, attributeFilter: ["class"] });
      cleanup.push(() => {
        sizes.disconnect();
        dark.disconnect();
      });
      void poll();
    }, (e) => setFailed(errorText(e)));

    return () => {
      live = false;
      window.clearTimeout(timer);
      for (const stop of cleanup) stop();
      term?.dispose();
    };
  }, [api, session.id]);

  const approve = async () => {
    setBusy(true);
    try {
      await api.termPaste(session.id);
      setState("running");
    } catch (e) {
      setFailed(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const close = async () => {
    if (!window.confirm("이 터미널 세션을 닫을까요? 안의 CLI가 종료됩니다.")) return;
    setBusy(true);
    try {
      await api.termClose(session.id);
      onClosed?.();
    } catch (e) {
      setFailed(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className={`flex h-full min-h-0 flex-1 flex-col bg-chat-canvas ${className ?? ""}`} role="group" aria-label={title ?? `${session.provider} 터미널`}>
      <div className="flex shrink-0 items-center gap-2 border-b px-3 py-1.5 text-2xs text-muted-foreground">
        <span aria-live="polite">
          {STATE_WORD[state] ?? state}
          {state === "exited" && exitCode !== null ? ` (코드 ${exitCode})` : ""}
        </span>
        {retrying && <span className="min-w-0 flex-1 truncate">연결을 다시 시도하는 중</span>}
        {inputBroken && (
          <span className="min-w-0 flex-1 truncate text-destructive">
            입력을 보내지 못했습니다
            <button
              type="button"
              className="ml-2 rounded underline underline-offset-2 focus-ring"
              onClick={() => {
                queueRef.current?.reset();
                setInputBroken(false);
              }}
            >
              다시 시도
            </button>
          </span>
        )}
        {failed && <span className="min-w-0 flex-1 truncate text-destructive">{failed}</span>}
        {!canDrive && <span className="min-w-0 flex-1 truncate">{TERM_DESKTOP_ONLY}</span>}
        <span className="flex-1" />
        {state === "awaiting-paste" && (
          <button type="button" disabled={busy || !canDrive} title={canDrive ? undefined : TERM_DESKTOP_ONLY} onClick={() => void approve()} className={BUTTON_SECONDARY}>
            붙여넣기 승인
          </button>
        )}
        {state !== "exited" && (
          <button type="button" disabled={busy || !canDrive} title={canDrive ? undefined : TERM_DESKTOP_ONLY} onClick={() => void close()} className={BUTTON_GHOST}>
            닫기
          </button>
        )}
      </div>
      <div ref={host} className="min-h-0 flex-1 bg-chat-canvas p-1 text-chat-foreground" />
    </div>
  );
}
