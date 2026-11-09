import { useEffect, useState, type FormEvent } from "react";
import type { Project } from "../../data/projects.ts";
import type { Backend, ScheduleJob, ScheduleRequest } from "../../platform/backend/index.ts";
import { BUTTON_PRIMARY, BUTTON_SECONDARY } from "../../ui/controls.tsx";

export function Automation({ api, projects, current }: { api: Backend; projects: Project[]; current?: string }) {
  const [jobs, setJobs] = useState<ScheduleJob[]>([]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [request, setRequest] = useState<ScheduleRequest>({ name: "", project: current ?? projects[0]?.path ?? "", provider: "claude", text: "", start: "", repeat: "once", clean: true, autoDispatch: true });
  const refresh = () => api.schedules().then(setJobs, (e: unknown) => setError(String(e)));
  useEffect(() => { void refresh(); }, [api]);
  const submit = async (event: FormEvent) => {
    event.preventDefault(); setBusy(true); setError("");
    try { await api.scheduleCreate(request); setRequest((r) => ({ ...r, name: "", text: "" })); await refresh(); }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };
  const remove = async (id: string) => {
    setBusy(true); setError("");
    try { await api.scheduleDelete(id); await refresh(); }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };
  const toggle = async (job: ScheduleJob) => {
    setBusy(true); setError("");
    try { await api.scheduleSetEnabled(job.id, !job.enabled); await refresh(); }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };
  const field = "w-full rounded-md border bg-background px-2.5 py-2 text-sm focus-ring";
  return <section className="flex min-h-0 min-w-0 flex-1 flex-col">
    <header className="border-b bg-tab-bar px-4 py-2 text-sm font-medium">자동화</header>
    <div className="scrollbar-sleek min-h-0 flex-1 overflow-y-auto bg-chat-canvas"><div className="mx-auto max-w-3xl space-y-6 p-5">
      <p className="text-xs text-muted-foreground">Project O를 닫아도 예약이 실행됩니다. PC가 켜져 있고 Windows에 로그인되어 있어야 합니다.</p>
      {error && <p role="alert" className="rounded-md border border-destructive/40 p-3 text-xs text-destructive">{error}</p>}
      <form onSubmit={(e) => void submit(e)} className="space-y-3 rounded-lg border bg-card p-4">
        <h2 className="text-sm font-semibold">예약 추가</h2>
        <label className="block space-y-1 text-xs">이름<input required maxLength={80} value={request.name} onChange={(e) => setRequest({ ...request, name: e.target.value })} className={field} /></label>
        <label className="block space-y-1 text-xs">프로젝트<select required value={request.project} onChange={(e) => setRequest({ ...request, project: e.target.value })} className={field}><option value="">선택하세요</option>{projects.map((p) => <option key={p.path} value={p.path}>{p.name}</option>)}</select></label>
        <label className="block space-y-1 text-xs">총괄에게 전달할 요청<textarea required maxLength={12000} rows={3} value={request.text} onChange={(e) => setRequest({ ...request, text: e.target.value })} className={field} /></label>
        <div className="grid grid-cols-2 gap-3"><label className="space-y-1 text-xs">시작 시간<input required type="datetime-local" value={request.start} onChange={(e) => setRequest({ ...request, start: e.target.value })} className={field} /></label><label className="space-y-1 text-xs">반복<select value={request.repeat} onChange={(e) => setRequest({ ...request, repeat: e.target.value as ScheduleRequest["repeat"] })} className={field}><option value="once">한 번</option><option value="daily">매일</option><option value="weekly">매주</option></select></label></div>
        <div className="grid grid-cols-2 gap-3"><label className="space-y-1 text-xs">총괄<select value={request.provider} onChange={(e) => setRequest({ ...request, provider: e.target.value as ScheduleRequest["provider"] })} className={field}><option value="claude">Claude</option><option value="codex">Codex</option></select></label><label className="space-y-1 text-xs">CLI 프로필<select value={request.clean ? "clean" : "own"} onChange={(e) => setRequest({ ...request, clean: e.target.value === "clean" })} className={field}><option value="clean">Project O 프로필</option><option value="own">기존 프로필</option></select></label></div>
        <label className="flex items-start gap-2 text-xs"><input type="checkbox" checked={request.autoDispatch} onChange={(e) => setRequest({ ...request, autoDispatch: e.target.checked })} />정책 검사 후 작업 분배까지 자동 진행 <span className="text-muted-foreground">(하루 예산과 실행 제한 적용)</span></label>
        <button type="submit" disabled={busy || !projects.length} className={BUTTON_PRIMARY}>예약 저장</button>
      </form>
      <section aria-labelledby="schedules-title"><div className="mb-2 flex items-center justify-between"><h2 id="schedules-title" className="text-sm font-semibold">등록된 예약</h2><button type="button" onClick={() => void refresh()} className={BUTTON_SECONDARY}>새로고침</button></div>
        {jobs.length === 0 ? <p className="text-xs text-muted-foreground">등록된 예약이 없습니다.</p> : <ul className="divide-y rounded-lg border">{jobs.map((job) => <li key={job.id} className="flex items-start gap-3 p-3"><div className="min-w-0 flex-1"><p className="text-sm font-medium">{job.name}</p><p className="break-words text-xs text-muted-foreground">{job.project} · {job.start.replace("T", " ")} · {job.repeat === "once" ? "한 번" : job.repeat === "daily" ? "매일" : "매주"} · {job.provider}</p><p className="mt-1 text-xs text-muted-foreground">{job.enabled ? "사용 중" : "일시 중지"} · {job.autoDispatch ? "조건 충족 시 자동 분배" : "설계까지만"}{job.last && ` · 최근 ${job.last.ok ? (job.autoDispatch ? "분배 시작" : "설계 완료") : "실행 실패"} ${new Date(job.last.at).toLocaleString()}`}</p>{job.last?.error && <p className="mt-1 break-words text-xs text-destructive">{job.last.error}</p>}</div><button type="button" disabled={busy} className={BUTTON_SECONDARY} onClick={() => void toggle(job)}>{job.enabled ? "일시 중지" : "다시 켜기"}</button><button type="button" disabled={busy} className={BUTTON_SECONDARY} onClick={() => void remove(job.id)}>삭제</button></li>)}</ul>}
      </section>
    </div></div>
  </section>;
}
