import { useState, type FormEvent } from "react";
import type { RunView } from "../../../../src/core/snapshot.ts";
import { errorText } from "../../lib/format.ts";
import { currentBackend, type FindingRow, type Issue } from "../../platform/backend/index.ts";

const field = "w-full rounded-md border bg-background px-2 py-1.5 text-sm focus-ring";
const button = "rounded-md border px-2.5 py-1.5 text-xs hover:bg-accent focus-ring disabled:opacity-50";

/** Local findings are linked to GitHub issues only after a person checks the candidates. */
export function IssuesPanel({ runs }: { runs: RunView[] }) {
  const api = currentBackend();
  const [repo, setRepo] = useState("");
  const [loadedRepo, setLoadedRepo] = useState("");
  const [rows, setRows] = useState<FindingRow[]>([]);
  const [runId, setRunId] = useState("");
  const [cardId, setCardId] = useState("");
  const [sha, setSha] = useState("");
  const [summary, setSummary] = useState("");
  const [reproduction, setReproduction] = useState("");
  const [evidence, setEvidence] = useState("");
  const [query, setQuery] = useState("");
  const [candidates, setCandidates] = useState<Issue[]>([]);
  const [truncated, setTruncated] = useState(false);
  const [findingId, setFindingId] = useState("");
  const [verifyRunId, setVerifyRunId] = useState("");
  const [verifyCardId, setVerifyCardId] = useState("");
  const [verifyEvidence, setVerifyEvidence] = useState("");
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("");
  const [error, setError] = useState("");
  const currentRun = runs.find((run) => run.runId === runId);
  const verifyRun = runs.find((run) => run.runId === verifyRunId);
  const selectFinding = (id: string) => {
    setFindingId(id); setVerifyRunId(""); setVerifyCardId(""); setVerifyEvidence("");
  };
  const work = async (fn: () => Promise<void>) => {
    setBusy(true); setError(""); setStatus("");
    try { await fn(); } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  };
  const load = async () => { setRows(await api.issuesList(repo.trim())); setLoadedRepo(repo.trim()); };
  const record = (event: FormEvent) => {
    event.preventDefault();
    void work(async () => {
      await api.issuesRecord({ repo: repo.trim(), runId, cardId, sha: sha.trim(), summary: summary.trim(), reproduction: reproduction.trim(), evidence: evidence.split("\n").map((line) => line.trim()).filter(Boolean), classification: "unclassified", issue: null });
      setSummary(""); setReproduction(""); setEvidence("");
      try { await load(); } catch (e) { throw new Error(`발견은 저장됐지만 목록을 읽지 못했습니다: ${String(e)}`); }
      setStatus("발견을 저장했습니다.");
    });
  };
  const classify = (classification: string, issue: Issue | null) => void work(async () => {
    if (!findingId) throw new Error("분류할 발견을 선택하세요.");
    await api.issuesClassify(Number(findingId), classification, issue && { number: issue.number, updatedAt: issue.updatedAt });
    try { await load(); } catch (e) { throw new Error(`분류는 저장됐지만 목록을 읽지 못했습니다: ${String(e)}`); }
    setStatus("분류를 저장했습니다.");
  });
  const resolve = (reopen: boolean) => void work(async () => {
    if (!findingId) throw new Error("발견을 선택하세요.");
    await api.issuesResolve(Number(findingId), reopen ? null : { runId: verifyRunId, cardId: verifyCardId, evidence: verifyEvidence.trim() });
    setVerifyRunId(""); setVerifyCardId(""); setVerifyEvidence("");
    try { await load(); } catch (e) { throw new Error(`상태는 저장됐지만 목록을 읽지 못했습니다: ${String(e)}`); }
    setStatus(reopen ? "발견을 다시 열었습니다." : "검증 근거와 로컬 종결을 기록했습니다.");
  });
  return <section className="space-y-4 text-sm" aria-label="이슈와 발견 관리">
    <div><h2 className="font-medium">이슈와 발견</h2><p className="mt-1 text-xs text-muted-foreground">발견은 이 PC에 저장됩니다. GitHub 이슈는 조회만 하며 변경하지 않습니다.</p></div>
    <label className="block space-y-1"><span>저장소 (소유자/이름)</span><input className={field} value={repo} disabled={busy} onChange={(e) => { setRepo(e.target.value); setLoadedRepo(""); setRows([]); setCandidates([]); setTruncated(false); selectFinding(""); }} placeholder="owner/repo" /></label>
    <button className={button} type="button" disabled={busy || !repo.trim()} onClick={() => void work(load)}>저장된 발견 불러오기</button>
    {busy && <p role="status" className="text-xs text-muted-foreground">처리 중…</p>}{error && <p role="alert" className="text-xs text-destructive">{error}</p>}{status && <p role="status" className="text-xs text-muted-foreground">{status}</p>}
    <details className="rounded-md border p-3"><summary className="cursor-pointer focus-ring">발견 기록</summary>
      <form className="mt-3 space-y-3" onSubmit={record}>
        <label className="block space-y-1"><span>실행</span><select className={field} value={runId} required disabled={busy} onChange={(e) => { setRunId(e.target.value); setCardId(""); }}><option value="">실행 선택</option>{runs.map((run) => <option key={run.runId} value={run.runId}>{run.title} · {run.runId}</option>)}</select></label>
        <label className="block space-y-1"><span>카드</span><select className={field} value={cardId} required disabled={busy || !currentRun} onChange={(e) => setCardId(e.target.value)}><option value="">카드 선택</option>{currentRun?.cards.map((card) => <option key={card.id} value={card.id}>{card.label} · {card.id}</option>)}</select></label>
        <label className="block space-y-1"><span>발견 당시 전체 커밋 SHA</span><input className={field} value={sha} onChange={(e) => setSha(e.target.value)} required disabled={busy} /></label>
        <label className="block space-y-1"><span>요약</span><input className={field} value={summary} onChange={(e) => setSummary(e.target.value)} required disabled={busy} /></label>
        <label className="block space-y-1"><span>재현과 관찰 결과</span><textarea className={field} value={reproduction} onChange={(e) => setReproduction(e.target.value)} required disabled={busy} /></label>
        <label className="block space-y-1"><span>증거 참조 (한 줄에 하나)</span><textarea className={field} value={evidence} onChange={(e) => setEvidence(e.target.value)} required disabled={busy} /></label>
        <button className={button} type="submit" disabled={busy || !repo.trim()}>미분류로 저장</button>
      </form>
    </details>
    {loadedRepo && <section className="space-y-2"><h3 className="font-medium">{loadedRepo}의 발견</h3>
      {rows.length === 0 ? <p className="text-xs text-muted-foreground">저장된 발견이 없습니다.</p> : <label className="block space-y-1"><span>분류할 발견</span><select className={field} value={findingId} disabled={busy} onChange={(e) => selectFinding(e.target.value)}><option value="">발견 선택</option>{rows.map(({ id, finding }) => <option key={id} value={id}>#{id} · {finding.summary} · {finding.classification}{finding.issue ? ` · 이슈 #${finding.issue.number}` : ""}</option>)}</select></label>}
      {findingId && <div className="space-y-1 text-xs text-muted-foreground">{rows.filter((row) => String(row.id) === findingId).map(({ id, finding }) => <div key={id}><p className="whitespace-pre-wrap">{finding.reproduction}</p><p className="mt-1 break-all">{finding.runId} / {finding.cardId} · {finding.sha}</p><p className="mt-1 whitespace-pre-wrap">증거: {finding.evidence.join(" · ")}</p><p className="mt-1">{finding.resolution ? `로컬 종결 · ${finding.resolution.runId} / ${finding.resolution.cardId} · ${finding.resolution.evidence}` : "미종결"}</p></div>)}</div>}
      <div className="flex flex-wrap gap-2"><button className={button} disabled={busy || !findingId} onClick={() => classify("unclassified", null)}>미분류</button><button className={button} disabled={busy || !findingId} onClick={() => classify("external", null)}>외부 장애</button><button className={button} disabled={busy || !findingId} onClick={() => classify("contention", null)}>자원 경합</button></div>
      {findingId && <details className="rounded-md border p-3"><summary className="cursor-pointer focus-ring">검증 후 로컬 종결</summary><p className="mt-2 text-xs text-muted-foreground">카드의 성공은 수정 증명이 아닙니다. 결과를 직접 확인하고 근거를 남겨 주세요. GitHub 이슈는 변경하지 않습니다.</p>
        <div className="mt-2 space-y-2"><label className="block space-y-1"><span>검증 실행</span><select className={field} value={verifyRunId} disabled={busy} onChange={(e) => { setVerifyRunId(e.target.value); setVerifyCardId(""); setVerifyEvidence(""); }}><option value="">실행 선택</option>{runs.map((run) => <option key={run.runId} value={run.runId}>{run.title} · {run.runId}</option>)}</select></label>
          <label className="block space-y-1"><span>검증 카드</span><select className={field} value={verifyCardId} disabled={busy || !verifyRun} onChange={(e) => { setVerifyCardId(e.target.value); setVerifyEvidence(""); }}><option value="">카드 선택</option>{verifyRun?.cards.map((card) => <option key={card.id} value={card.id}>{card.label} · {card.id}</option>)}</select></label>
          <label className="block space-y-1"><span>확인한 근거</span><textarea className={field} value={verifyEvidence} disabled={busy} onChange={(e) => setVerifyEvidence(e.target.value)} /></label>
          <div className="flex gap-2"><button className={button} disabled={busy || !verifyRunId || !verifyCardId || !verifyEvidence.trim()} onClick={() => resolve(false)}>검증 근거로 종결</button><button className={button} disabled={busy || !rows.find((row) => String(row.id) === findingId)?.finding.resolution} onClick={() => resolve(true)}>다시 열기</button></div>
        </div></details>}
    </section>}
    <section className="space-y-2"><h3 className="font-medium">GitHub 이슈 검색</h3><div className="flex gap-2"><label className="min-w-0 flex-1"><span className="sr-only">이슈 검색어</span><input className={field} value={query} disabled={busy} onChange={(e) => { setQuery(e.target.value); setCandidates([]); setTruncated(false); }} placeholder="검색어" /></label><button className={button} disabled={busy || !repo.trim() || !query.trim()} onClick={() => void work(async () => { const result = await api.issuesSearch(repo.trim(), query.trim()); setCandidates(result.issues); setTruncated(result.possiblyTruncated); if (!result.issues.length) setStatus("검색 결과가 없습니다."); })}>검색</button></div>
      {truncated && <p role="status" className="text-xs text-status-warning-text">결과가 더 있을 수 있습니다. 검색어를 좁혀 주세요.</p>}
      <ul className="space-y-2">{candidates.map((issue) => <li key={issue.number} className="rounded-md border p-2"><p className="break-words">#{issue.number} {issue.title}</p><p className="text-xs text-muted-foreground">{issue.state} · 수정 {issue.updatedAt}</p><div className="mt-2 flex flex-wrap gap-2"><button className={button} disabled={busy} onClick={() => void work(() => api.openExternal(issue.url))}>GitHub에서 확인</button><button className={button} disabled={busy || !findingId || loadedRepo !== repo.trim()} onClick={() => classify("related", issue)}>관련 이슈로 연결</button><button className={button} disabled={busy || !findingId || loadedRepo !== repo.trim()} onClick={() => classify("duplicate", issue)}>중복으로 연결</button></div></li>)}</ul>
    </section>
  </section>;
}
