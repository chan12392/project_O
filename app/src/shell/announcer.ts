import { useEffect, useRef, useState } from "react";

import type { RunView } from "../../../src/core/snapshot.ts";
import { settingsNow } from "../data/settings.ts";
import { STATE_WORD } from "../lib/format.ts";
import { newlyFinished } from "./finishedCards.ts";

/**
 * Settings → 알림: a desktop notification for a finished card while the window is in the background.
 * In the desktop app `Notification` is the Tauri notification plugin's (Windows toasts); in the
 * browser preview it is the browser's own.
 */
function notify(title: string, body: string) {
  if (!settingsNow().notify || typeof Notification === "undefined" || Notification.permission !== "granted" || document.hasFocus()) return;
  try {
    new Notification(title, { body });
  } catch {
    // Some webviews only allow notifications from a service worker; the screen reader line still speaks.
  }
}

/**
 * Says out loud (and, if enabled, notifies) when a card of any O run finishes, not only the run on
 * screen (a Claude Code session only while it is on screen); cards that had already finished when
 * their run first showed up stay quiet.
 */
export function useAnnouncer(runs: readonly { view: RunView }[], current: RunView | undefined) {
  const [text, setText] = useState("");
  const seen = useRef(new Set<string>());
  const known = useRef(new Set<string>());
  useEffect(() => {
    for (const { run, card } of newlyFinished(runs.map((r) => r.view), seen.current, known.current, current?.runId)) {
      const said = `${card.label} 카드 ${STATE_WORD[card.state]}`;
      setText(run.runId === current?.runId ? said : `${run.title} · ${said}`);
      notify(run.title, said);
    }
  }, [runs, current]);
  return text;
}
