import { useEffect, useState } from "react";

import { backend, type TokenGroup, type TokenReport } from "../../platform/backend/index.ts";
import { compact } from "../../lib/format.ts";

const api = backend(new URLSearchParams(window.location.search).get("source") === "demo");

type Load = { state: "loading" } | { state: "error"; message: string } | { state: "ready"; report: TokenReport };

/** How much less (or more) the shared-context clean agents spend per card than the default harness. */
function verdict({ baseline, shared }: TokenReport["compare"]): string | undefined {
  if (!baseline || !shared || baseline.perCard.total === 0) return undefined;
  const pct = Math.round(((shared.perCard.total - baseline.perCard.total) / baseline.perCard.total) * 100);
  if (pct === 0) return "깨끗한+세션 공유와 기본 하네스의 카드당 토큰이 같습니다";
  return `깨끗한+세션 공유가 기본 하네스보다 카드당 ${Math.abs(pct)}% ${pct < 0 ? "적게" : "많이"} 씁니다`;
}

/** Cards finish while the dashboard stays open: read the report again this often while the page is visible. */
const REFRESH_MS = 15_000;

/**
 * Tokens per card for each way of running cards (`o tokens`), for one project:
 * the default harness against clean agents that get only the shared context.
 * Read again every REFRESH_MS while visible (and on coming back), keeping the last report on screen meanwhile.
 */
export function TokenCard({ project }: { project?: string }) {
  const [load, setLoad] = useState<Load>({ state: "loading" });
  useEffect(() => {
    let live = true;
    let inflight = false;
    let timer: number | undefined;
    let last = "";
    setLoad({ state: "loading" });
    const schedule = () => {
      window.clearTimeout(timer);
      if (live && !document.hidden) timer = window.setTimeout(read, REFRESH_MS);
    };
    const read = () => {
      if (inflight) return;
      inflight = true;
      api.tokens(project).then(
        (report) => {
          // An unchanged report keeps the same state, so a quiet refresh re-renders nothing.
          const text = JSON.stringify(report);
          if (live && text !== last) {
            last = text;
            setLoad({ state: "ready", report });
          }
        },
        (e: unknown) => {
          last = "";
          if (live) setLoad({ state: "error", message: e instanceof Error ? e.message : String(e) });
        },
      ).finally(() => {
        inflight = false;
        schedule();
      });
    };
    const onVisibility = () => (document.hidden ? window.clearTimeout(timer) : read());
    read();
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      live = false;
      window.clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [project]);

  return (
    <section aria-labelledby="tokens-title" aria-busy={load.state === "loading"} className="rounded-xl border bg-card p-3.5 text-card-foreground">
      <h3 id="tokens-title" className="flex items-baseline gap-2 text-2xs font-semibold tracking-wider text-muted-foreground uppercase">
        토큰
        <span className="font-normal tracking-normal normal-case">카드당 평균 · 실행 방식별</span>
      </h3>
      {load.state === "loading" ? (
        <p className="mt-2 text-xs text-muted-foreground">불러오는 중…</p>
      ) : load.state === "error" ? (
        <p className="mt-2 text-xs text-muted-foreground">토큰 기록을 읽지 못했습니다: {load.message}</p>
      ) : load.report.groups.length === 0 ? (
        <p className="mt-2 text-xs text-muted-foreground">아직 토큰을 보고한 카드가 없습니다. 카드가 끝나면 CLI가 보고한 사용량이 여기에 모입니다.</p>
      ) : (
        <Groups report={load.report} />
      )}
    </section>
  );
}

function Groups({ report }: { report: TokenReport }) {
  const note = verdict(report.compare);
  return (
    <>
      <div className="mt-2 overflow-x-auto">
        <table className="w-full text-2xs tabular-nums">
          <thead className="text-muted-foreground">
            <tr className="text-right">
              <th scope="col" className="py-1 text-left font-medium">방식</th>
              <th scope="col" className="py-1 pl-3 font-medium">카드</th>
              <th scope="col" className="py-1 pl-3 font-medium">새 입력</th>
              <th scope="col" className="py-1 pl-3 font-medium">캐시</th>
              <th scope="col" className="py-1 pl-3 font-medium">출력</th>
              <th scope="col" className="py-1 pl-3 font-medium">합계</th>
            </tr>
          </thead>
          <tbody className="divide-y">
            {report.groups.map((g) => (
              <Row key={g.key} group={g} />
            ))}
          </tbody>
        </table>
      </div>
      {note && <p className="mt-2 text-xs">{note}</p>}
    </>
  );
}

function Row({ group: g }: { group: TokenGroup }) {
  const providers = g.byProvider.map((p) => `${p.provider} ${p.cards}장 · ${compact.format(p.perCard.total)}`).join(", ");
  return (
    <tr className="text-right">
      <th scope="row" className="py-1.5 text-left font-normal" title={providers}>
        <span className="text-xs text-foreground">{g.label}</span> <span className="text-muted-foreground">실행 {g.runs}</span>
      </th>
      <td className="py-1.5 pl-3">{g.cards}</td>
      <td className="py-1.5 pl-3">{compact.format(g.perCard.input)}</td>
      <td className="py-1.5 pl-3">{compact.format(g.perCard.cached)}</td>
      <td className="py-1.5 pl-3">{compact.format(g.perCard.output)}</td>
      <td className="py-1.5 pl-3 font-semibold">{compact.format(g.perCard.total)}</td>
    </tr>
  );
}
