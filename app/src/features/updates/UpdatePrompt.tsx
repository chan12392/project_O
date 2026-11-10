import { useEffect, useRef, useState } from "react";
import { errorText } from "../../lib/format.ts";
import type { Backend } from "../../platform/backend/index.ts";
import { BUTTON_PRIMARY, BUTTON_SECONDARY } from "../../ui/controls.tsx";
import { settingsNow, updateSettings } from "../../data/settings.ts";
import { UPDATE_EVENT, type UpdateRelease } from "../../platform/updates.ts";

/** Mounted once; native dialog supplies focus containment and makes the app behind it inert. */
export function UpdatePrompt({ api }: { api: Backend }) {
  const [release, setRelease] = useState<UpdateRelease | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [skip, setSkip] = useState(false);
  const dialog = useRef<HTMLDialogElement>(null);
  const laterButton = useRef<HTMLButtonElement>(null);
  const installing = useRef(false);
  const dismissed = useRef(false);

  useEffect(() => {
    const show = (event: Event) => {
      if (!installing.current) setRelease((event as CustomEvent<UpdateRelease>).detail);
    };
    window.addEventListener(UPDATE_EVENT, show);
    const timer = api.kind === "tauri" ? window.setInterval(() => {
      if (installing.current) return;
      void api.pendingUpdate().then((next) => {
        if (next && next.tag !== settingsNow().skippedUpdate) setRelease((current) => current ?? next);
      }).catch(() => {}); // Background checks are intentionally silent.
    }, 5_000) : undefined;
    return () => { window.removeEventListener(UPDATE_EVENT, show); clearInterval(timer); };
  }, [api]);

  useEffect(() => {
    if (!release) return;
    const previous = document.activeElement as HTMLElement | null;
    const modal = dialog.current;
    dismissed.current = false;
    setError("");
    setSkip(false);
    modal?.showModal();
    laterButton.current?.focus();
    return () => { modal?.close(); previous?.focus(); };
  }, [release]);

  const later = () => {
    dismissed.current = true;
    if (busy) void api.cancelUpdate().catch(() => {});
    if (skip && release) updateSettings({ skippedUpdate: release.tag });
    setRelease(null);
  };
  const install = async () => {
    if (!release || installing.current) return;
    installing.current = true;
    setBusy(true);
    setError("");
    try { await api.installUpdate(release.tag); }
    catch (e) { if (!dismissed.current) setError(errorText(e)); }
    finally { installing.current = false; setBusy(false); }
  };

  return (
    <dialog ref={dialog} aria-labelledby="update-title" aria-describedby="update-description" onCancel={(event) => { event.preventDefault(); later(); }}
      onKeyDown={(event) => {
        if (event.key !== "Tab") return;
        const controls = Array.from(event.currentTarget.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), [tabindex="0"]'));
        const first = controls[0], last = controls[controls.length - 1];
        if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
        if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
      }}
      className="m-auto w-[min(36rem,calc(100vw-2rem))] max-h-[calc(100dvh-2rem)] overflow-y-auto rounded-xl border bg-background p-6 text-foreground backdrop:bg-black/60">
      {release && <>
        <h2 id="update-title" className="text-lg font-semibold">새 버전이 있습니다</h2>
        <p className="mt-2 break-all font-mono text-xs text-muted-foreground">{release.tag} · {new Date(release.publishedAt).toLocaleDateString("ko-KR")}</p>
        <div tabIndex={0} aria-label="릴리스 변경 사항" className="mt-5 max-h-64 overflow-y-auto whitespace-pre-wrap break-words text-sm leading-relaxed focus-ring">
          {release.notes || "등록된 변경 사항이 없습니다."}
        </div>
        <p id="update-description" className="mt-5 text-xs leading-relaxed text-muted-foreground">설치 파일을 받은 뒤 앱이 닫히고 설치 화면이 열립니다. 진행 중인 작업을 마친 뒤 업데이트해 주세요. 설치가 끝나면 앱을 다시 열 수 있습니다.</p>
        <label className="mt-4 flex items-center gap-2 text-[13px]">
          <input type="checkbox" checked={skip} onChange={(event) => setSkip(event.target.checked)} disabled={busy} className="size-4 accent-primary focus-ring" />
          이 버전 건너뛰기
        </label>
        <p role="status" className="mt-3 text-xs text-muted-foreground">{busy ? "설치 파일을 내려받고 확인하는 중… 나중에를 누르면 설치를 취소합니다." : ""}</p>
        {error && <p role="alert" className="mt-3 whitespace-pre-wrap break-words text-sm text-destructive">{error}</p>}
        <div className="mt-5 flex flex-wrap justify-end gap-2">
          <button ref={laterButton} type="button" onClick={later} className={BUTTON_SECONDARY}>나중에</button>
          <button type="button" onClick={() => void install()} disabled={busy || skip} className={BUTTON_PRIMARY}>{busy ? "다운로드 중…" : "지금 업데이트"}</button>
        </div>
      </>}
    </dialog>
  );
}
