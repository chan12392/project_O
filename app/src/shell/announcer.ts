import { useEffect, useRef, useState } from "react";

import type { RunView } from "../../../src/core/snapshot.ts";
import { settingsNow } from "../data/settings.ts";
import { STATE_WORD } from "../lib/format.ts";

/** Settings → 알림: a desktop notification for a finished card while the window is in the background. */
function notify(title: string, body: string) {
  if (!settingsNow().notify || typeof Notification === "undefined" || Notification.permission !== "granted" || document.hasFocus()) return;
  try {
    new Notification(title, { body });
  } catch {
    // Some webviews only allow notifications from a service worker; the screen reader line still speaks.
  }
}

/** Says out loud (and, if enabled, notifies) when a card finishes while its run is watched; cards that had already finished stay quiet. */
export function useAnnouncer(run: RunView | undefined) {
  const [text, setText] = useState("");
  const seen = useRef(new Set<string>());
  const runs = useRef(new Set<string>());
  useEffect(() => {
    if (!run) return;
    const first = !runs.current.has(run.runId);
    runs.current.add(run.runId);
    for (const card of run.cards) {
      const key = `${run.runId}/${card.id}`;
      if (card.endedAt && !seen.current.has(key)) {
        seen.current.add(key);
        if (first) continue;
        const said = `${card.label} 카드 ${STATE_WORD[card.state]}`;
        setText(said);
        notify(run.title, said);
      }
    }
  }, [run]);
  return text;
}
