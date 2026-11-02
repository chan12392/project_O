import { History, X } from "lucide-react";
import { useEffect, useId, useMemo, useState } from "react";

import { isSessionRun } from "../../../../src/core/attention.ts";
import { folderKey, importCandidates, type ProjectList } from "../../../../src/core/projects.ts";
import type { RunView } from "../../../../src/core/snapshot.ts";
import { importOffered, markImportOffered, projectsApi } from "../../data/projects.ts";
import { registerFolder } from "./ProjectForms.tsx";

interface Candidate {
  path: string;
  /** O runs and Claude Code sessions found there. */
  runs: number;
  sessions: number;
}

/**
 * The first-run offer (ADR-0009): with nothing registered yet, the folders history
 * worked in that the machine accepts as projects, newest first. Shown once.
 */
export function useImportOffer(list: ProjectList | undefined, runs: RunView[], threads: string[], enabled: boolean) {
  const [offered, setOffered] = useState(importOffered);
  const [candidates, setCandidates] = useState<Candidate[]>([]);
  const wanted = enabled && !offered && list !== undefined && !list.projects.some((p) => p.kind === "registered");
  const paths = useMemo(
    () => (wanted && list ? importCandidates([...runs.map((r) => r.project), ...threads], list.projects, list.root) : []),
    [wanted, list, runs, threads],
  );
  const key = paths.join("\n");
  useEffect(() => {
    const api = projectsApi();
    if (!key || !api) return setCandidates([]);
    let gone = false;
    void api.projectsCheck(key.split("\n")).then(
      (answers) => {
        if (gone) return;
        const ok = answers.filter((a) => a.ok).map((a) => a.path);
        setCandidates(
          ok.map((path) => ({
            path,
            runs: runs.filter((r) => folderKey(r.project) === folderKey(path) && !isSessionRun(r)).length,
            sessions: runs.filter((r) => folderKey(r.project) === folderKey(path) && isSessionRun(r)).length,
          })),
        );
      },
      () => !gone && setCandidates([]),
    );
    return () => {
      gone = true;
    };
    // Counts follow the paths; a new poll of the same runs needs no new check.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  const dismiss = () => {
    markImportOffered();
    setOffered(true);
  };
  return { candidates: wanted ? candidates : [], dismiss };
}

/** "기록에서 찾은 프로젝트": tick the folders to register, with what the root does from now on. */
export function ImportCard({ root, candidates, onDone, onDismiss }: { root: string; candidates: Candidate[]; onDone: (first?: string) => void; onDismiss: () => void }) {
  const titleId = useId();
  // Folders O dispatched to are ticked; folders seen only in Claude Code sessions wait for a tick.
  const [picked, setPicked] = useState(() => new Set(candidates.filter((c) => c.runs > 0).map((c) => c.path)));
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState<{ path: string; reason: string }[]>([]);

  const register = async () => {
    setBusy(true);
    const refused: { path: string; reason: string }[] = [];
    let first: string | undefined;
    for (const path of candidates.map((c) => c.path).filter((p) => picked.has(p))) {
      try {
        const listed = await registerFolder(path);
        first ??= listed;
      } catch (e) {
        refused.push({ path, reason: e instanceof Error ? e.message : String(e) });
      }
    }
    setBusy(false);
    setFailed(refused);
    if (refused.length === 0) onDone(first);
  };

  return (
    <section aria-labelledby={titleId} className="bloom mx-4 mt-3 flex max-h-[min(26rem,55%)] shrink-0 flex-col rounded-xl border bg-background shadow-sm">
      <header className="flex items-start gap-2 px-4 pt-3">
        <History className="mt-0.5 size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
        <div className="min-w-0 flex-1">
          <h2 id={titleId} className="text-[13px] font-semibold">
            기록에서 찾은 프로젝트
          </h2>
          <p className="mt-0.5 text-xs text-muted-foreground">
            왼쪽 목록에는 프로젝트 루트 <span className="font-mono break-all">{root}</span>의 폴더(그 안의 test는 묶음으로)와 등록한 폴더만 보입니다. 예전 작업 폴더 중 계속 볼 것을 골라
            등록하세요. 고르지 않은 기록도 지워지지 않습니다.
          </p>
        </div>
        <button type="button" onClick={onDismiss} aria-label="기록에서 찾은 프로젝트 닫기 (다시 보이지 않음)" title="닫기 (다시 보이지 않음)" className="flex size-6 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground focus-ring">
          <X className="size-3.5" aria-hidden="true" />
        </button>
      </header>
      <fieldset className="scrollbar-sleek mt-2 min-h-0 flex-1 overflow-y-auto px-4" aria-describedby={titleId}>
        <legend className="sr-only">등록할 폴더</legend>
        {candidates.map((c) => {
          const reason = failed.find((f) => f.path === c.path)?.reason;
          return (
            <label key={c.path} className="flex cursor-pointer items-start gap-2 rounded-md px-1 py-1.5 hover:bg-accent/50">
              <input
                type="checkbox"
                checked={picked.has(c.path)}
                onChange={(e) =>
                  setPicked((s) => {
                    const next = new Set(s);
                    if (e.target.checked) next.add(c.path);
                    else next.delete(c.path);
                    return next;
                  })
                }
                className="mt-0.5 size-3.5 shrink-0 accent-foreground focus-ring"
              />
              <span className="min-w-0 flex-1">
                <span className="block truncate font-mono text-2xs" title={c.path}>
                  {c.path}
                </span>
                <span className="block text-3xs text-muted-foreground">
                  {[c.runs > 0 && `O 실행 ${c.runs}`, c.sessions > 0 && `Claude Code 세션 ${c.sessions}`].filter(Boolean).join(" · ") || "총괄 대화"}
                </span>
                {reason && (
                  <span role="alert" className="block text-3xs text-destructive">
                    {reason}
                  </span>
                )}
              </span>
            </label>
          );
        })}
      </fieldset>
      <footer className="flex items-center gap-2 border-t px-4 py-2.5">
        <button
          type="button"
          onClick={() => void register()}
          disabled={busy || picked.size === 0}
          className="flex h-7 items-center rounded-md bg-primary px-2.5 text-xs font-medium whitespace-nowrap text-primary-foreground hover:bg-primary/90 focus-ring disabled:cursor-not-allowed disabled:opacity-50"
        >
          {busy ? "등록하는 중…" : `고른 ${picked.size}개 등록`}
        </button>
        <button type="button" onClick={onDismiss} disabled={busy} className="flex h-7 items-center rounded-md border px-2.5 text-xs whitespace-nowrap hover:bg-accent focus-ring disabled:opacity-50">
          등록하지 않기
        </button>
        <span className="ml-auto text-3xs text-muted-foreground">한 번만 묻습니다</span>
      </footer>
    </section>
  );
}
