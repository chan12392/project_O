import { CircleAlert, CircleCheck, FileText, FolderOpen, Image as ImageIcon, OctagonX, Play, Wrench } from "lucide-react";
import { useEffect, useId, useRef, useState, type FormEvent } from "react";

import { EFFORT_WORD, LEAD_NAME as NAMES, fileSize, leadProgressSummary, leadReadProblem, leadToolLabel, planIsStale, taskIsLong, type LeadItem } from "../../../../src/core/lead.ts";
import { failedBeforeCards, type RunView } from "../../../../src/core/snapshot.ts";
import { PROVIDER_NAME, timeOf } from "../../lib/format.ts";
import type { LeadState } from "../../data/useLead.ts";
import { useSettings } from "../../data/settings.ts";
import { useBudget } from "../../data/useBudget.ts";
import { BUTTON_PRIMARY } from "../../ui/controls.tsx";
import { Markdown } from "./Markdown.tsx";
import { ImageScope } from "./ImageGallery.tsx";
import { Notice, RunCard, useFollowBottom } from "./Thread.tsx";

/** Thread events may name an agent this build does not know; it shows as its id. */
const LEAD_NAME: Record<string, string> = NAMES;
const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e));

interface Props {
  project: string | undefined;
  lead: LeadState;
  onCompose: () => void;
  /** A folder chosen when there is no project yet; resolves once its thread exists. */
  onStartProject: (path: string) => Promise<void>;
}

/** The real lead chat (ADR-0008): messages, the lead's answers, its plans and what they started. */
export function LeadThread({ project, lead, onCompose, onStartProject }: Props) {
  const scroller = useFollowBottom([lead.items, lead.busy, lead.thread?.partial]);
  const [stopError, setStopError] = useState("");
  const seen = useRef<{ thread: string; replies: Set<string> } | undefined>(undefined);
  const threadId = lead.thread?.threadId;
  useEffect(() => {
    if (!threadId) { seen.current = undefined; return; }
    const replies = new Set(lead.items.flatMap((item, i) => item.kind === "reply" ? [`${item.at}-${i}`] : []));
    seen.current = { thread: threadId, replies };
  }, [threadId, lead.items]);
  const known = seen.current;
  const lastUser = lead.items.findLastIndex((item) => item.kind === "user" || item.kind === "worker");
  const answering = lead.busy && !lead.items.slice(lastUser + 1).some((item) => item.kind === "reply" || item.kind === "error");
  const progressSummary = leadProgressSummary(lead.thread?.progressCounts);
  return (
    <ImageScope.Provider value={project ? { project } : undefined}><div ref={scroller} className="scrollbar-sleek min-h-0 flex-1 overflow-y-auto bg-chat-canvas">
      <div className="mx-auto flex max-w-3xl flex-col gap-6 px-6 py-6 text-sm text-chat-foreground">
        {lead.thread && <div className="flex justify-end"><button type="button" className="rounded border px-2 py-1 text-xs focus-ring" onClick={() => void lead.stopAll().then(() => setStopError(""), (e) => setStopError(errorText(e)))}>모두 중지</button></div>}
        {stopError && <p role="alert" className="text-xs text-destructive">모두 중지하지 못했습니다: {stopError}</p>}
        {!project && <ProjectStart onStart={onStartProject} />}
        {project && !lead.loaded && !lead.failed && (
          <div className="flex flex-col gap-4" aria-busy="true" aria-label="대화 불러오는 중">
            <div className="shimmer ml-auto h-14 w-3/5 rounded-lg" aria-hidden="true" />
            <div className="shimmer h-20 w-4/5 rounded-lg" aria-hidden="true" />
          </div>
        )}
        {project && lead.failed && !lead.loaded && (
          <Notice title="총괄 대화를 읽지 못했습니다" action="다시 시도" onAction={() => void lead.refresh()}>
            <span className="whitespace-pre-wrap">{leadReadProblem(lead.failed)}</span>
          </Notice>
        )}
        {project && lead.loaded && lead.items.length === 0 && !lead.busy && (
          <Notice title="아직 총괄과 나눈 대화가 없습니다" action="첫 요청 쓰기" onAction={onCompose}>
            할 일을 보내면 총괄이 계획을 제안합니다. 계획을 확인한 뒤 작업을 분배할 수 있습니다.
          </Notice>
        )}
        {lead.items.map((item, i) => (
          <Item key={`${threadId}-${item.kind}-${item.at}-${i}`} item={item} animate={Boolean(item.kind === "reply" && threadId && known && known.thread === threadId && !known.replies.has(`${item.at}-${i}`))} />
        ))}
        {answering && lead.thread?.partial && <div aria-label="작성 중인 답변"><Markdown text={lead.thread.partial} /></div>}
        {project && lead.failed && lead.loaded && (
          // Read once, then a check failed: what shows may be old. One line, not the whole notice.
          <p role="status" className="flex items-center gap-2 text-xs text-status-warning-text">
            <CircleAlert className="size-3.5 shrink-0" aria-hidden="true" />
            <span className="min-w-0 truncate" title={lead.failed}>
              최근 대화를 다시 읽지 못했습니다. 보이는 내용이 오래됐을 수 있습니다.
            </span>
            <button type="button" onClick={() => void lead.refresh()} className="shrink-0 rounded underline underline-offset-2 hover:text-chat-foreground focus-ring">
              다시 시도
            </button>
          </p>
        )}
        {answering && (
          <div role="status" className="space-y-1 text-xs text-chat-foreground-faint">
            {progressSummary && <p className="flex items-center gap-1.5"><Wrench className="size-3 shrink-0" aria-hidden="true" /><span>{progressSummary}</span></p>}
            {(lead.thread?.progress ?? []).map((step, i) => (
              <p key={`${step.name}-${step.summary}-${i}`} className="truncate" title={step.summary}>{leadToolLabel(step.name, step.summary)}</p>
            ))}
            <p>총괄 {LEAD_NAME[lead.thread?.busy ?? ""] ?? ""} · 작업 중…</p>
          </div>
        )}
      </div>
    </div></ImageScope.Provider>
  );
}

function Item({ item, animate }: { item: LeadItem; animate: boolean }) {
  switch (item.kind) {
    case "worker":
      return <article aria-label="작업 카드 메시지" className="rounded-lg border px-3.5 py-2.5"><p className="mb-1 text-xs text-chat-foreground-faint">카드 {item.cardId} · {item.runId} · <time dateTime={item.at}>{timeOf(item.at)}</time></p><p className="whitespace-pre-wrap break-words">{item.text}</p></article>;
    case "user":
      return (
        <div className="ml-auto max-w-[85%] rounded-lg border border-chat-user-border bg-chat-user-surface px-3.5 py-2.5">
          <Markdown text={item.text} />
          {item.attachments.length > 0 && (
            <ul aria-label="첨부" className="mt-2 flex flex-wrap gap-1.5">
              {item.attachments.map((a) => (
                <li key={a.path} className="flex max-w-56 items-center gap-1.5 rounded-md border bg-background/40 px-2 py-1 text-xs" title={a.path}>
                  {a.image ? <ImageIcon className="size-3.5 shrink-0 opacity-70" aria-hidden="true" /> : <FileText className="size-3.5 shrink-0 opacity-70" aria-hidden="true" />}
                  <span className="min-w-0 truncate">{a.name}</span>
                  <span className="shrink-0 text-chat-foreground-faint tabular-nums">{fileSize(a.size)}</span>
                </li>
              ))}
            </ul>
          )}
          <p className="mt-1.5 text-right text-2xs text-chat-foreground-faint">
            {LEAD_NAME[item.provider] ?? item.provider}
            {item.model ? ` · ${item.model}` : ""}
            {item.effort ? ` · ${EFFORT_WORD[item.effort] ?? item.effort}` : ""} · <time dateTime={item.at}>{timeOf(item.at)}</time>
          </p>
        </div>
      );
    case "reply":
      return (
        <article aria-label="총괄의 답">
          <p className="mb-1.5 text-xs text-chat-foreground-faint">
            총괄 {LEAD_NAME[item.provider] ?? item.provider} <time dateTime={item.at}>{timeOf(item.at)}</time>
          </p>
          {item.reportedFiles && item.reportedFiles.length > 0 && <details className="mb-2 text-xs text-chat-foreground-faint"><summary className="cursor-pointer rounded py-1 focus-ring">변경 보고된 파일 {item.reportedFiles.length}{item.reportedFiles.length === 100 ? "개 이상" : "개"} · 줄 수 미제공</summary><ul className="mt-1 space-y-1">{item.reportedFiles.map((path) => <li key={path} className="break-all">{path}</li>)}</ul></details>}
          <ReplyText text={item.text} animate={animate} />
        </article>
      );
    case "error":
      return (
        <p role="alert" className="flex items-start gap-2 rounded-lg border border-destructive/40 px-3.5 py-2.5 text-sm">
          <CircleAlert className="mt-0.5 size-4 shrink-0 text-destructive" aria-hidden="true" />
          <span className="min-w-0 whitespace-pre-wrap">{item.text}</span>
        </p>
      );
    case "followup":
      return <p role="status" className="text-xs text-chat-foreground-faint">{item.cardId} 에이전트의 후속 지시를 접수했습니다 · {item.runId}</p>;
    case "plan":
      return <p className="text-xs text-chat-foreground-faint">{item.title} · {item.runId ? `분배한 작업 ${item.runId}` : "오른쪽 설계에서 최신 분배안을 확인하세요."}</p>;
  }
}

function ReplyText({ text, animate }: { text: string; animate: boolean }) {
  const chars = Array.from(text);
  const [visible, setVisible] = useState(() => animate && !window.matchMedia("(prefers-reduced-motion: reduce)").matches ? 0 : chars.length);
  useEffect(() => {
    if (visible >= chars.length) return;
    const start = performance.now();
    const duration = Math.min(2000, Math.max(400, chars.length * 5));
    const timer = window.setInterval(() => {
      const next = Math.min(chars.length, Math.ceil(chars.length * (performance.now() - start) / duration));
      setVisible(next);
      if (next >= chars.length) window.clearInterval(timer);
    }, 45);
    return () => window.clearInterval(timer);
  }, [text]);
  return <Markdown text={chars.slice(0, visible).join("")} />;
}

/** A proposed plan, its policy result, and explicit dispatch or retry. */
export function PlanCard({ plan, lead, run, onOpenCard }: { plan: Extract<LeadItem, { kind: "plan" }>; lead: LeadState; run?: RunView; onOpenCard: (runId: string, cardId: string) => void }) {
  const { clean } = useSettings();
  const budget = useBudget();
  const over = budget?.providers.filter((entry) => entry.state === "over" && plan.cards.some((card) => card.provider === entry.provider)) ?? [];
  const [state, setState] = useState<{ busy?: boolean; error?: string }>({});
  const titleId = useId();
  const stale = planIsStale(lead.items, plan.planId);
  // Passed when saved, then refused by the runner's own check right before the start.
  const refused = run !== undefined && failedBeforeCards(run) && Boolean(run.blocked?.length);
  const retryable = run?.status === "failed";
  const budgetViolations = (run?.blocked ?? (!plan.ok ? plan.violations : [])).filter((violation) => violation.rule === "daily-budget");
  const budgetBlocked = budgetViolations.length > 0;
  const budgetCards = budgetViolations.map((violation) => violation.cardId);
  const budgetProviders = plan.cards.filter((card) => budgetCards.includes(card.id) || budgetCards.includes(undefined)).map((card) => card.provider);
  const budgetResolved = budget && (budget.enabled === false || (budgetProviders.length > 0 && budgetProviders.every((provider) => budget.providers.some((entry) => entry.provider === provider && ["ok", "behind", "farBehind", "off"].includes(entry.state)))));
  const isolated = run?.cards.some((card) => card.log.some((entry) => entry.kind === "tool" && entry.text.startsWith("O 작업 폴더 ")));
  const dispatch = async () => {
    setState({ busy: true });
    try {
      await lead.dispatch(plan.planId, clean);
      setState({});
    } catch (e) {
      setState({ error: errorText(e) });
    }
  };

  return (
    <section className="@container overflow-hidden rounded-xl border bg-card text-card-foreground" aria-labelledby={titleId}>
      <header className="flex items-center justify-between gap-3 border-b px-4 py-3">
        <div className="min-w-0">
          <h2 id={titleId} className="truncate text-sm font-medium" title={plan.title}>
            {plan.title}
          </h2>
          <p className="text-2xs text-muted-foreground">
            총괄이 제안한 계획 · 작업 {plan.cards.length}개 · <time dateTime={plan.at}>{timeOf(plan.at)}</time>
          </p>
        </div>
        {refused ? (
          <span className="flex shrink-0 items-center gap-1 rounded-full border border-destructive/40 px-2 py-0.5 text-2xs text-destructive">
            <OctagonX className="size-3" aria-hidden="true" />
            실행 직전 차단
          </span>
        ) : plan.ok ? (
          <span className="flex shrink-0 items-center gap-1 rounded-full border px-2 py-0.5 text-2xs text-status-success">
            <CircleCheck className="size-3" aria-hidden="true" />
            계획 당시 정책 통과
          </span>
        ) : (
          <span className="flex shrink-0 items-center gap-1 rounded-full border border-destructive/40 px-2 py-0.5 text-2xs text-destructive">
            <OctagonX className="size-3" aria-hidden="true" />
            계획 당시 정책 차단
          </span>
        )}
      </header>
      <PlanDetails plan={plan} />
      {!plan.runId && over.length > 0 && <div role="note" className="border-t px-4 py-3 text-xs text-destructive">
        <p>최근 확인한 예산이 소진되어 실행이 차단될 수 있습니다. 실행 직전에 다시 검사합니다.</p>
        {over.map((entry) => <p key={entry.provider} className="mt-1">{entry.message}</p>)}
      </div>}
      {!plan.runId && budgetBlocked && !budgetResolved && <p role="note" className="border-t px-4 py-3 text-xs text-status-warning-text">하루 예산이 초기화된 뒤 다시 시도하거나, 총괄에게 예산이 남은 다른 에이전트로 설계를 변경해 달라고 요청하세요.</p>}
      <footer className="flex flex-wrap items-center gap-3 border-t px-4 py-3">
        {!plan.runId && plan.autoRunReason && <p role="status" className="w-full text-xs text-muted-foreground">{plan.autoRunReason}</p>}
        {plan.runId ? (
          run && failedBeforeCards(run) ? (
            <p role="alert" className="text-xs text-destructive">
              실행 {plan.runId}는 카드를 시작하기 전에 막혔습니다. 아래 이유를 확인하세요.
            </p>
          ) : run ? (
            <p className="text-xs text-muted-foreground">실행 {plan.runId}의 진행은 아래에서 볼 수 있습니다.{isolated && " 결과는 각 작업 폴더에 보존되며 원본에 자동으로 합쳐지지 않습니다."}</p>
          ) : (
            <p role="status" className="text-xs text-muted-foreground">
              실행 {plan.runId}를 시작했습니다. 첫 기록을 기다리는 중…
            </p>
          )
        ) : plan.problems.length === 0 ? (
          <>
            <p role="note" className="w-full text-xs text-status-warning-text">Git 프로젝트는 미커밋 변경을 정리한 뒤 분배하세요. 결과는 작업별 폴더에 남으며 자동으로 합쳐지지 않습니다.</p>
            <button type="button" onClick={dispatch} disabled={state.busy || lead.busy || Boolean(lead.failed) || stale} aria-describedby={`${titleId}-how`} className={BUTTON_PRIMARY}>
              <Play className="size-3.5" aria-hidden="true" />
              {state.busy ? "분배하는 중…" : plan.ok ? "작업 분배" : "다시 확인하고 분배"}
            </button>
            <p id={`${titleId}-how`} className="min-w-0 flex-1 text-xs text-muted-foreground">
              {stale ? "이후 대화가 있습니다. 총괄에게 최신 설계를 다시 요청하세요." : `실행 직전에 정책${budget?.enabled === false ? "" : "(하루 예산 포함)"}을 다시 검사한 뒤 ${clean ? "project_O 프로필" : "내 설정"}로 작업을 시작합니다.`}
            </p>
          </>
        ) : (
          <p className="text-xs text-muted-foreground">
            {plan.problems.length > 0 && plan.violations.length === 0 ? "형식이 올바르지 않은 계획" : "정책에 막힌 계획"}은 실행하지 않습니다. 총괄에게 사유를 알려 다시
            계획해 달라고 하세요.
          </p>
        )}
        {retryable && <>
          {budgetBlocked && !budgetResolved && <p role="note" className="w-full text-xs text-status-warning-text">하루 예산이 초기화된 뒤 다시 시도하거나, 총괄에게 예산이 남은 다른 에이전트로 설계를 변경해 달라고 요청하세요.</p>}
          {(!budgetBlocked || run?.blocked?.some((violation) => violation.rule !== "daily-budget")) && <p role="note" className="w-full text-xs text-status-warning-text">아래 실행 기록에서 실패 원인을 확인하고 해결한 뒤 다시 실행하세요.</p>}
          {!failedBeforeCards(run) && <p className="w-full text-xs text-status-warning-text">다시 실행하면 완료된 작업도 새 작업 폴더에서 다시 시작합니다.</p>}
          <button type="button" onClick={dispatch} disabled={state.busy || lead.busy || Boolean(lead.failed) || stale} className={BUTTON_PRIMARY}><Play className="size-3.5" aria-hidden="true" />{state.busy ? "재실행 준비 중…" : "다시 실행"}</button>
          {stale && <p className="text-xs text-muted-foreground">대화가 변경되어 최신 설계가 필요합니다.</p>}
        </>}
        {state.error && (
          <p role="alert" className="w-full text-xs whitespace-pre-wrap text-destructive">
            {state.error}
          </p>
        )}
      </footer>
      {run && (
        <div className="border-t p-3">
          <RunCard run={run} onOpenCard={(card) => onOpenCard(run.runId, card)} />
        </div>
      )}
    </section>
  );
}

/**
 * What a plan proposes: its cards with their dependencies, shape problems and policy violations.
 * Read-only, so an earlier plan stays readable after a newer one replaced it (#40); running is PlanCard's.
 */
export function PlanDetails({ plan }: { plan: Extract<LeadItem, { kind: "plan" }> }) {
  return (
    <>
      {plan.cards.length > 0 && (
        <div className="relative overflow-x-auto">
          {/* relative: the sr-only caption stays inside the chat instead of stretching the page. */}
          <table className="w-full text-left text-xs">
            <caption className="sr-only">{plan.title}의 작업 분배</caption>
            <thead className="text-2xs text-muted-foreground">
              <tr className="border-b">
                <th scope="col" className="px-4 py-2 font-normal">
                  작업
                </th>
                <th scope="col" className="px-2 py-2 font-normal">
                  에이전트
                </th>
                <th scope="col" className="px-2 py-2 font-normal @max-md:hidden">
                  모델 · 노력
                </th>
                <th scope="col" className="px-2 py-2 pr-4 font-normal">
                  할 일
                </th>
              </tr>
            </thead>
            <tbody>
              {plan.cards.map((card) => (
                <tr key={card.id} className="border-b align-top last:border-b-0">
                  <th scope="row" className="px-4 py-2.5 font-normal whitespace-nowrap">
                    <span className="block font-mono text-foreground">{card.id}</span>
                    {card.role && <span className="block text-2xs text-muted-foreground">{card.role}</span>}
                  </th>
                  <td className="px-2 py-2.5 whitespace-nowrap">{PROVIDER_NAME[card.provider as keyof typeof PROVIDER_NAME] ?? card.provider}</td>
                  <td className="px-2 py-2.5 whitespace-nowrap text-muted-foreground @max-md:hidden">
                    {card.model || "기본 모델"}
                    {card.effort ? ` · ${EFFORT_WORD[card.effort] ?? card.effort}` : ""}
                  </td>
                  <td className="px-2 py-2.5 pr-4">
                    <CardTask task={card.task} />
                    {Boolean(card.depends_on?.length) && <p className="mt-1 text-2xs text-muted-foreground">선행: {card.depends_on?.join(", ")}</p>}
                    <p className="mt-1 text-2xs text-muted-foreground break-all">자원: {card.resources?.length ? card.resources.map((r) => `${r.kind === "project-path" ? "파일" : "외부"} ${r.key} (${r.mode === "read" ? "읽기" : "쓰기"})`).join(", ") : "미선언 · 독점 실행"}</p>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {plan.problems.length > 0 && (
        <div role="note" className="border-t px-4 py-3 text-xs" aria-label="계획 형식 위반">
          <p className="flex items-center gap-1.5 font-medium text-destructive">
            <CircleAlert className="size-3.5 shrink-0" aria-hidden="true" />
            계획의 형식이 올바르지 않아 일부를 보이지 않습니다. 실행할 수 없습니다.
          </p>
          <ul className="mt-1.5 list-disc space-y-0.5 pl-5 text-muted-foreground">
            {plan.problems.map((p, i) => (
              <li key={i}>{p}</li>
            ))}
          </ul>
        </div>
      )}
      {!plan.ok && plan.violations.length > 0 && (
        <ul className="space-y-1 border-t px-4 py-3 text-xs" aria-label="정책 위반">
          {plan.violations.map((v, i) => (
            <li key={i} className="flex gap-2">
              <span className="shrink-0 font-mono text-destructive">[{v.rule}]</span>
              <span className="min-w-0">
                {v.cardId && <span className="font-mono">{v.cardId}: </span>}
                {v.message}
              </span>
            </li>
          ))}
        </ul>
      )}
    </>
  );
}

/**
 * A card's task: three lines, and for a longer one a "전체 보기" that opens it in place. The plan
 * runs once, so the whole task must be readable before 실행 — by keyboard and touch too, not
 * only in a mouse tooltip.
 */
function CardTask({ task }: { task: string }) {
  const [open, setOpen] = useState(false);
  const long = taskIsLong(task);
  return (
    <>
      <p className={`min-w-32 whitespace-pre-wrap [overflow-wrap:anywhere] text-foreground/90 ${long && !open ? "line-clamp-3" : ""}`}>{task}</p>
      {long && (
        <button
          type="button"
          onClick={() => setOpen((o) => !o)}
          aria-expanded={open}
          className="mt-1 rounded text-2xs text-muted-foreground underline underline-offset-2 hover:text-foreground focus-ring"
        >
          {open ? "접기" : "전체 보기"}
        </button>
      )}
    </>
  );
}

/**
 * The folder the lead will read, typed as an absolute path: shown here when there is no
 * project yet, and exported for a "새 프로젝트" entry elsewhere (the sidebar's, not wired here).
 */
export function ProjectStart({ onStart }: { onStart: (path: string) => Promise<void> }) {
  const [path, setPath] = useState("");
  const [state, setState] = useState<{ busy?: boolean; error?: string }>({});
  const absolute = /^(?:[A-Za-z]:[\\/]|\/)/.test(path.trim());
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!absolute) return setState({ error: "C:\\work\\app 처럼 절대 경로를 적어 주세요" });
    setState({ busy: true });
    try {
      await onStart(path.trim());
      setState({});
    } catch (err) {
      setState({ error: errorText(err) });
    }
  };
  return (
    <section className="rounded-xl border bg-card p-6 text-card-foreground" aria-labelledby="project-start-title">
      <h2 id="project-start-title" className="flex items-center gap-2 text-sm font-medium">
        <FolderOpen className="size-4 text-muted-foreground" aria-hidden="true" />
        총괄이 맡을 프로젝트 폴더
      </h2>
      <p className="mt-1.5 text-sm text-muted-foreground">총괄은 이 폴더를 읽기만 하고 작업카드 계획을 제안합니다. 홈 폴더 자체나 O의 상태 폴더는 쓸 수 없습니다.</p>
      <form className="mt-4 flex gap-2 max-sm:flex-col" onSubmit={submit} noValidate>
        <label htmlFor="project-path" className="sr-only">
          프로젝트 폴더 경로
        </label>
        <input
          id="project-path"
          value={path}
          onChange={(e) => {
            setPath(e.target.value);
            if (state.error) setState({});
          }}
          spellCheck={false}
          placeholder="C:\work\my-app"
          aria-invalid={Boolean(state.error)}
          aria-describedby={state.error ? "project-path-error" : undefined}
          className="h-8 min-w-0 flex-1 rounded-md border bg-background px-2.5 font-mono text-xs outline-none focus-ring placeholder:text-muted-foreground"
        />
        <button type="submit" disabled={state.busy || !path.trim()} className={BUTTON_PRIMARY}>
          {state.busy ? "여는 중…" : "이 폴더로 시작"}
        </button>
      </form>
      {state.error && (
        <p id="project-path-error" role="alert" className="mt-2 text-xs text-destructive">
          {state.error}
        </p>
      )}
    </section>
  );
}
