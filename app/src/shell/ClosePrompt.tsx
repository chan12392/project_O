import { useEffect, useRef, useState } from "react";

import { settingsNow, updateSettings, type CloseAction } from "../data/settings.ts";
import { BUTTON_PRIMARY, BUTTON_SECONDARY } from "../ui/controls.tsx";
import type { SettingsSection } from "../features/settings/sections/index.ts";

type Activity = { cards: number; leads: number; runs: number };
type Prompt = { kind: "choose" } | { kind: "checking" } | { kind: "confirm"; activity?: Activity; error?: string };

export function ClosePrompt({ onSettings }: { onSettings: (section: SettingsSection) => void }) {
  const [prompt, setPrompt] = useState<Prompt>();
  const [remember, setRemember] = useState(false);
  const [busy, setBusy] = useState(false);
  const dialog = useRef<HTMLDialogElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  const pending = useRef<Exclude<CloseAction, "ask"> | undefined>(undefined);
  const invoke = window.__TAURI_INTERNALS__?.invoke;

  async function finishQuit() {
    if (pending.current) updateSettings({ closeAction: pending.current });
    await invoke?.("quit_app");
  }

  async function quit() {
    if (!invoke) return;
    setBusy(true);
    setPrompt({ kind: "checking" });
    try {
      const activity = await invoke("quit_activity") as Activity;
      if (activity.cards || activity.leads || activity.runs) setPrompt({ kind: "confirm", activity });
      else await finishQuit();
    } catch {
      setPrompt({ kind: "confirm", error: "작업 중인 에이전트를 확인하지 못했습니다." });
    } finally { setBusy(false); }
  }

  async function choose(action: Exclude<CloseAction, "ask">) {
    if (!invoke || busy) return;
    pending.current = remember ? action : undefined;
    if (action === "quit") return quit();
    try { await invoke("hide_main"); if (pending.current) updateSettings({ closeAction: pending.current }); setPrompt(undefined); }
    catch { setPrompt({ kind: "confirm", error: "트레이로 숨기지 못했습니다. 앱을 종료할지 확인해 주세요." }); }
  }

  useEffect(() => {
    if (!invoke) return;
    const close = () => {
      pending.current = undefined;
      setRemember(false);
      const action = settingsNow().closeAction;
      if (action === "ask") setPrompt({ kind: "choose" });
      else if (action === "tray") void invoke("hide_main").catch(() => setPrompt({ kind: "confirm", error: "트레이로 숨기지 못했습니다." }));
      else void quit();
    };
    const tray = (event: Event) => {
      pending.current = undefined;
      const action = (event as CustomEvent<string>).detail;
      if (action === "quit") void quit();
      if (action === "settings") onSettings("general");
      if (action === "about") onSettings("about");
    };
    window.addEventListener("project-o:close-requested", close);
    window.addEventListener("project-o:tray-action", tray);
    return () => { window.removeEventListener("project-o:close-requested", close); window.removeEventListener("project-o:tray-action", tray); };
  });

  useEffect(() => {
    if (!prompt) return;
    const previous = document.activeElement as HTMLElement | null;
    dialog.current?.showModal();
    cancel.current?.focus();
    return () => { dialog.current?.close(); previous?.focus(); };
  }, [Boolean(prompt)]);

  useEffect(() => { if (prompt && !busy) cancel.current?.focus(); }, [prompt?.kind, busy]);

  return <dialog ref={dialog} aria-labelledby="close-title" onCancel={(event) => { event.preventDefault(); if (!busy) setPrompt(undefined); }} className="m-auto w-[min(28rem,90vw)] rounded-xl border bg-card p-5 text-card-foreground shadow-lg backdrop:bg-black/50">
    {prompt && <>
      <h2 id="close-title" className="text-base font-semibold">{prompt.kind === "choose" ? "창을 닫으시겠습니까?" : prompt.kind === "checking" ? "작업 상태 확인 중" : "앱을 종료하시겠습니까?"}</h2>
      {prompt.kind === "checking" ? <p role="status" className="mt-2 text-sm text-muted-foreground">작업 중인 에이전트를 확인하고 있습니다.</p> : prompt.kind === "choose" ? <>
        <p className="mt-2 text-sm text-muted-foreground">트레이에 숨기면 작업을 계속 볼 수 있고, 완전 종료하면 앱이 닫힙니다.</p>
        <label className="mt-4 flex items-center gap-2 text-sm"><input type="checkbox" checked={remember} onChange={(event) => setRemember(event.target.checked)} /> 선택 기억하기</label>
      </> : <p className="mt-2 whitespace-pre-wrap text-sm text-muted-foreground">
        {prompt.error ?? `진행 상태의 실행 ${prompt.activity?.runs ?? 0}개, 작업 중인 카드 ${prompt.activity?.cards ?? 0}개, 총괄 ${prompt.activity?.leads ?? 0}개가 있습니다.`}{"\n"}카드는 계속 실행될 수 있으며 앱이 종료되면 총괄의 답변은 중단될 수 있습니다.
      </p>}
      <div className="mt-5 flex flex-wrap justify-end gap-2">
        <button ref={cancel} type="button" className={BUTTON_SECONDARY} disabled={busy} onClick={() => setPrompt(undefined)}>취소</button>
        {prompt.kind === "choose" ? <>
          <button type="button" className={BUTTON_SECONDARY} disabled={busy} onClick={() => void choose("tray")}>트레이로 숨기기</button>
          <button type="button" className={BUTTON_PRIMARY} disabled={busy} onClick={() => void choose("quit")}>완전 종료</button>
        </> : prompt.kind === "confirm" && <button type="button" className={BUTTON_PRIMARY} disabled={busy} onClick={() => void finishQuit().catch(() => setPrompt({ kind: "confirm", error: "앱을 종료하지 못했습니다." }))}>그래도 종료</button>}
      </div>
    </>}
  </dialog>;
}
