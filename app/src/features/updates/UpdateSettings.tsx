import { useEffect, useState } from "react";
import { errorText } from "../../lib/format.ts";
import type { Backend } from "../../platform/backend/index.ts";
import { BUTTON_SECONDARY, SettingsRow } from "../../ui/controls.tsx";
import { UNINSTALL_KEEP, UNINSTALL_PURGE, UPDATE_EVENT, updateStatusLine, type UpdateStatus } from "../../platform/updates.ts";

export function UpdateSettings({ api }: { api: Backend }) {
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState("");
  const [error, setError] = useState(false);
  const [status, setStatus] = useState<UpdateStatus>();
  useEffect(() => {
    if (api.kind !== "tauri") return;
    let live = true;
    void api.updateStatus().then((s) => live && setStatus(s), () => {});
    return () => { live = false; };
  }, [api]);
  const check = async () => {
    setBusy(true); setNote(""); setError(false);
    try {
      const result = await api.checkUpdate();
      setNote(result.message);
      if (result.release) window.dispatchEvent(new CustomEvent(UPDATE_EVENT, { detail: result.release }));
    } catch (e) { setError(true); setNote(errorText(e)); }
    finally { setBusy(false); }
  };
  if (api.kind !== "tauri" || !status?.platform) return <p className="py-4 text-xs text-muted-foreground">업데이트와 제거 안내는 데스크톱 앱의 운영체제를 확인한 뒤 표시합니다.</p>;
  if (status.platform !== "windows") return <div className="divide-y">
    <div className="py-4">
      <p className="text-[13px] font-medium">앱 업데이트</p>
      <p className="mt-2 text-xs leading-relaxed text-muted-foreground">{status.platform === "macos" ? "macOS 자동 업데이트는 아직 지원하지 않습니다. 앱을 종료한 뒤 새 project_O.app으로 교체해 주세요." : "이 운영체제의 자동 업데이트는 아직 지원하지 않습니다."}</p>
    </div>
    <div className="py-4">
      <p className="text-[13px] font-medium">앱 제거</p>
      <p className="mt-2 text-xs leading-relaxed text-muted-foreground">{status.platform === "macos" ? "앱을 종료한 뒤 Finder에서 project_O.app을 휴지통으로 옮기세요. 기록·로그인·설정과 프로젝트는 그대로 남습니다." : "설치한 방법에 따라 앱을 제거해 주세요. 기록과 설정은 별도로 남습니다."}</p>
    </div>
  </div>;
  return <div className="divide-y">
    <SettingsRow label="앱 업데이트" description={<>
      데스크톱 앱은 시작 후와 6시간마다 새 테스트 릴리스를 확인합니다. 확인과 내려받기에는 GitHub CLI(gh) 설치와 로그인이 필요합니다.
      {status && <span className={`mt-2 block break-words ${status.gh === false || status.lastCheck?.error ? "text-foreground" : ""}`}>{updateStatusLine(status)}</span>}
      {note && <span role={error ? "alert" : "status"} className={`mt-2 block whitespace-pre-wrap break-words ${error ? "text-destructive" : ""}`}>{note}</span>}
    </>} control={<button type="button" onClick={() => void check()} disabled={busy} className={BUTTON_SECONDARY}>{busy ? "확인하는 중…" : "업데이트 확인"}</button>} />
    <UninstallCommand label="앱만 제거" command={UNINSTALL_KEEP} description="PowerShell에서 실행합니다. 실행 중인 앱을 닫고 제거하며, 기록·로그인·설정은 남깁니다." />
    <UninstallCommand label="앱과 데이터 모두 제거" command={UNINSTALL_PURGE} description="실행 기록·첨부·깨끗한 에이전트의 로그인·창 설정·거울 동기화 예약 작업도 삭제합니다. 평소 CLI 설정과 프로젝트는 보존합니다." />
  </div>;
}

function UninstallCommand({ label, command, description }: { label: string; command: string; description: string }) {
  const [note, setNote] = useState("");
  const copy = async () => {
    try { await navigator.clipboard.writeText(command); setNote("복사했습니다"); }
    catch { setNote("복사하지 못했습니다. 명령을 직접 선택해 복사해 주세요."); }
  };
  return <div className="py-4">
    <div className="flex items-center justify-between gap-4">
      <p className="text-[13px] font-medium">{label}</p>
      <button type="button" aria-label={`${label} 명령 복사`} onClick={() => void copy()} className={BUTTON_SECONDARY}>명령 복사</button>
    </div>
    <p className="mt-2 text-xs leading-relaxed text-muted-foreground">{description}</p>
    <code className="mt-3 block select-text whitespace-pre-wrap break-all rounded-md bg-muted p-3 font-mono text-xs leading-relaxed">{command}</code>
    {note && <p role="status" className="mt-2 text-xs text-muted-foreground">{note}</p>}
  </div>;
}
