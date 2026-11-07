import { Boxes, Folder, RefreshCw, Sparkles, Waypoints, X } from "lucide-react";
import { useCallback, useEffect, useState, type ReactNode } from "react";

import type { Project } from "../../data/projects.ts";
import { useReducedMotion } from "../../data/settings.ts";
import type { Backend, GraphNode, GraphSettings } from "../../platform/backend/index.ts";
import { ICON_BUTTON } from "../../lib/format.ts";
import { BUTTON_PRIMARY, BUTTON_SECONDARY, Switch } from "../../ui/controls.tsx";
import { DeepConfirm } from "./DeepConfirm.tsx";
import { Graph3D } from "./Graph3D.tsx";
import { useGraphView, type GraphScope } from "./useGraphView.ts";

const KIND_WORD: Record<GraphNode["kind"], string> = { root: "전체", project: "프로젝트", area: "영역", file: "파일" };
const GLOW_WORD: Record<NonNullable<GraphNode["glow"]>, string> = { active: "지금 바뀐 곳", path: "그 영향 경로" };

const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e));

/**
 * The Graph tab (ADR-0033): the overall graph branches into one graph per project.
 * Left column is "전체" with the listed projects under it; the canvas shows the 3D
 * graph of the picked scope; the toolbar holds the effect switches and actions.
 */
export function GraphScreen({ api, projects, current }: { api: Backend; projects: Project[]; current?: string }) {
  const [scope, setScope] = useState<GraphScope>({ scope: "overall" });
  const { view, error, loading, refresh } = useGraphView(api, scope);
  const [settings, setSettings] = useState<GraphSettings>();
  const [settingsError, setSettingsError] = useState<string>();
  const [busy, setBusy] = useState<"build" | "deep">();
  const [notice, setNotice] = useState<string>();
  const [detail, setDetail] = useState<GraphNode>();
  const [confirm, setConfirm] = useState(false);
  /** Which projects have an index, remembered from the last overall view for the list's badge. */
  const [indexed, setIndexed] = useState<Record<string, boolean>>({});
  const reducedMotion = useReducedMotion();
  const project = scope.scope === "project" ? scope.project : undefined;

  useEffect(() => {
    if (view?.scope !== "overall") return;
    setIndexed(Object.fromEntries(view.nodes.filter((n) => n.kind === "project").map((n) => [n.project ?? n.id.slice(2), n.indexed !== false])));
  }, [view]);

  useEffect(() => {
    let live = true;
    api.graphSettings().then(
      (s) => live && setSettings(s),
      (e) => live && setSettingsError(errorText(e)),
    );
    return () => {
      live = false;
    };
  }, [api]);

  const changeSettings = useCallback(
    async (change: Partial<GraphSettings>) => {
      setSettings((s) => (s ? { ...s, ...change } : s));
      try {
        setSettings(await api.graphSettings(change));
      } catch (e) {
        setNotice(`설정을 저장하지 못했습니다: ${errorText(e)}`);
        setSettings(await api.graphSettings().catch(() => undefined));
      }
    },
    [api],
  );

  const build = useCallback(async () => {
    if (!project || busy) return;
    setBusy("build");
    setNotice(undefined);
    try {
      const result = await api.graphBuild(project);
      setNotice(result.ok ? `인덱스를 만들었습니다. ${result.output}` : `인덱스를 만들지 못했습니다. ${result.output}`);
      await refresh();
    } catch (e) {
      setNotice(`인덱스를 만들지 못했습니다: ${errorText(e)}`);
    } finally {
      setBusy(undefined);
    }
  }, [api, project, busy, refresh]);

  const deep = useCallback(async () => {
    if (!project || !settings || busy) return;
    setBusy("deep");
    setNotice(undefined);
    try {
      const model = settings.deepProvider === "codex" ? settings.codexModel : settings.claudeModel;
      const result = await api.graphDeep(project, settings.deepProvider, model || undefined);
      const { input, output } = result.usage;
      const tokens = input != null && output != null ? `, 입력 ${input.toLocaleString()}·출력 ${output.toLocaleString()} 토큰` : "";
      setNotice(`Deep이 영역 ${result.areas.length}개의 이름과 요약을 만들었습니다 (${result.provider} ${result.model}${tokens}).`);
      await refresh();
    } catch (e) {
      setNotice(`Deep을 실행하지 못했습니다: ${errorText(e)}`);
    } finally {
      setBusy(undefined);
    }
  }, [api, project, settings, busy, refresh]);

  const onDeep = () => {
    if (!settings) return;
    if (settings.warnDeep) setConfirm(true);
    else void deep();
  };

  const onNode = useCallback(
    (node: GraphNode) => {
      if (scope.scope === "overall" && node.kind === "project") {
        const path = node.project ?? node.id.slice(2);
        setDetail(undefined);
        setScope({ scope: "project", project: path });
        return;
      }
      setDetail(node);
    },
    [scope],
  );

  const indexedOf = (path: string) => indexed[path];

  const notInstalled = view !== undefined && !view.grafly.installed;
  const deepModel = settings ? (settings.deepProvider === "codex" ? settings.codexModel : settings.claudeModel) : "";

  return (
    <section className="flex min-h-0 min-w-0 flex-1 flex-col" aria-label="Graph">
      <header className="flex h-9 shrink-0 items-center gap-3 border-b bg-tab-bar px-3 text-xs">
        <span className="font-medium">Graph</span>
        <ToolSwitch id="graph-effects" label="효과" checked={settings?.effects ?? true} disabled={!settings} onChange={(effects) => void changeSettings({ effects })} />
        <ToolSwitch id="graph-particles" label="흐름" checked={settings?.particles ?? true} disabled={!settings || reducedMotion} onChange={(particles) => void changeSettings({ particles })} />
        {reducedMotion && <span className="text-2xs text-muted-foreground">동작 줄이기 켜짐 — 흐름 끔</span>}
        <span className="ml-auto flex items-center gap-1.5">
          {view?.truncated && <span className="text-2xs text-muted-foreground">노드가 많아 일부만 표시합니다</span>}
          <button type="button" onClick={() => void refresh()} disabled={loading} aria-label="새로고침" title="새로고침" className={ICON_BUTTON}>
            <RefreshCw className={`size-3.5 ${loading ? "animate-spin" : ""}`} aria-hidden="true" />
          </button>
          {project && !notInstalled && (
            <button
              type="button"
              onClick={() => void build()}
              disabled={busy !== undefined}
              className={view && !view.indexed ? BUTTON_PRIMARY : BUTTON_SECONDARY}
            >
              <Boxes className="size-3.5" aria-hidden="true" />
              인덱스 만들기
            </button>
          )}
          {project && !notInstalled && (
            <button type="button" onClick={onDeep} disabled={busy !== undefined || !settings || !view?.indexed} title={view && !view.indexed ? "인덱스를 먼저 만드세요" : "LLM으로 영역 이름과 요약을 만듭니다"} className={BUTTON_SECONDARY}>
              <Sparkles className="size-3.5" aria-hidden="true" />
              Deep
            </button>
          )}
        </span>
      </header>
      {settingsError && (
        <p role="alert" className="shrink-0 border-b bg-destructive/10 px-4 py-1.5 text-xs text-destructive">
          Graph 설정을 읽지 못했습니다: {settingsError}
        </p>
      )}
      {notice && (
        <p role="status" className="flex shrink-0 items-start gap-2 border-b bg-muted/50 px-4 py-1.5 text-xs">
          <span className="min-w-0 flex-1 whitespace-pre-wrap">{notice}</span>
          <button type="button" onClick={() => setNotice(undefined)} aria-label="알림 닫기" className="shrink-0 rounded focus-ring">
            <X className="size-3.5" aria-hidden="true" />
          </button>
        </p>
      )}
      <div className="flex min-h-0 flex-1">
        <nav className="scrollbar-sleek w-48 shrink-0 overflow-y-auto border-r py-2 pl-2" aria-label="그래프 범위">
          <ScopeButton icon={<Waypoints className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />} active={scope.scope === "overall"} onClick={() => { setDetail(undefined); setScope({ scope: "overall" }); }}>
            전체
          </ScopeButton>
          <div className="ml-3 border-l pl-1">
            {projects.map((p) => (
              <ScopeButton
                key={p.path}
                icon={<Folder className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />}
                active={scope.scope === "project" && scope.project === p.path}
                missing={p.missing}
                title={p.path}
                badge={indexedOf(p.path) === false ? "인덱스 없음" : current === p.path ? "현재" : undefined}
                onClick={() => { setDetail(undefined); setScope({ scope: "project", project: p.path }); }}
              >
                {p.name}
              </ScopeButton>
            ))}
            {projects.length === 0 && <p className="px-2 py-1.5 text-2xs text-muted-foreground">목록에 프로젝트가 없습니다.</p>}
          </div>
        </nav>
        <div className="relative min-w-0 flex-1 bg-chat-canvas">
          {loading && <Center text="그래프를 불러오는 중…" />}
          {!loading && error && (
            <Center
              text={`그래프를 불러오지 못했습니다: ${error}`}
              action={
                <button type="button" onClick={() => void refresh()} className={BUTTON_SECONDARY}>
                  다시 시도
                </button>
              }
            />
          )}
          {!loading && !error && notInstalled && (
            <Center
              title="grafly가 설치되어 있지 않습니다"
              text="코드 그래프는 grafly가 만듭니다. O는 grafly를 내려받거나 설치하지 않습니다. grafly를 설치한 뒤 프로젝트에서 인덱스를 만들어 주세요."
            />
          )}
          {!loading && !error && view && !notInstalled && view.nodes.length === 0 && (
            <Center
              title={project ? "인덱스가 없습니다" : "인덱스된 프로젝트가 없습니다"}
              text={project ? "이 프로젝트의 코드 그래프를 보려면 먼저 인덱스를 만드세요." : "프로젝트에서 인덱스를 만들면 전체 그래프에 나타납니다."}
              action={
                project ? (
                  <button type="button" onClick={() => void build()} disabled={busy !== undefined} className={BUTTON_PRIMARY}>
                    <Boxes className="size-3.5" aria-hidden="true" />
                    인덱스 만들기
                  </button>
                ) : undefined
              }
            />
          )}
          {!loading && !error && view && !notInstalled && view.nodes.length > 0 && (
            <Graph3D view={view} effects={settings?.effects ?? true} particles={settings?.particles ?? true} reducedMotion={reducedMotion} onNodeClick={onNode} className="absolute inset-0" />
          )}
          {busy === "deep" && (
            <div role="status" className="absolute inset-0 z-10 grid place-items-center bg-background/60">
              <p className="rounded-lg border bg-card px-4 py-3 text-sm shadow-lg">
                Deep으로 영역 이름과 요약을 만드는 중…
                <span className="mt-1 block text-xs text-muted-foreground">몇 분 걸릴 수 있습니다.</span>
              </p>
            </div>
          )}
          {busy === "build" && (
            <div role="status" className="absolute inset-x-0 top-2 z-10 mx-auto w-fit rounded-md border bg-card px-3 py-1.5 text-xs shadow">
              인덱스를 만드는 중…
            </div>
          )}
          {view?.deep && !busy && (
            <p className="absolute bottom-2 left-2 rounded-md bg-card/80 px-2 py-1 text-2xs text-muted-foreground">
              Deep: {view.deep.provider} {view.deep.model} · {new Date(view.deep.at).toLocaleDateString()}
            </p>
          )}
          {detail && (
            <aside className="absolute right-2 top-2 z-10 w-64 rounded-lg border bg-card p-3 text-xs shadow-lg" aria-label="노드 정보">
              <header className="flex items-start gap-2">
                <span className="min-w-0 flex-1 font-medium">{detail.label}</span>
                <button type="button" onClick={() => setDetail(undefined)} aria-label="닫기" className="shrink-0 rounded focus-ring">
                  <X className="size-3.5" aria-hidden="true" />
                </button>
              </header>
              <dl className="mt-2 space-y-1 text-muted-foreground">
                <div className="flex gap-2"><dt className="w-12 shrink-0">종류</dt><dd>{KIND_WORD[detail.kind]}</dd></div>
                {detail.path && <div className="flex gap-2"><dt className="w-12 shrink-0">경로</dt><dd className="min-w-0 font-mono text-2xs break-all">{detail.path}</dd></div>}
                {detail.project && <div className="flex gap-2"><dt className="w-12 shrink-0">프로젝트</dt><dd className="min-w-0 font-mono text-2xs break-all">{detail.project}</dd></div>}
                {detail.glow && <div className="flex gap-2"><dt className="w-12 shrink-0">상태</dt><dd className="text-agent-working">{GLOW_WORD[detail.glow]}</dd></div>}
                {detail.kind === "project" && detail.indexed === false && <div className="flex gap-2"><dt className="w-12 shrink-0">인덱스</dt><dd>없음</dd></div>}
                <div className="flex gap-2"><dt className="w-12 shrink-0">크기</dt><dd>{detail.size}</dd></div>
                {detail.summary && <div className="flex gap-2"><dt className="w-12 shrink-0">요약</dt><dd className="min-w-0">{detail.summary}</dd></div>}
              </dl>
            </aside>
          )}
        </div>
      </div>
      {confirm && settings && project && (
        <DeepConfirm
          provider={settings.deepProvider}
          model={deepModel}
          onDone={(run) => {
            setConfirm(false);
            if (!run) return;
            if (run.dontAskAgain) void changeSettings({ warnDeep: false });
            void deep();
          }}
        />
      )}
    </section>
  );
}

/** A labelled switch for the graph toolbar (settings switches live in Settings → Graph). */
function ToolSwitch({ id, label, checked, disabled, onChange }: { id: string; label: string; checked: boolean; disabled?: boolean; onChange: (next: boolean) => void }) {
  return (
    <span className="flex items-center gap-1.5">
      <span id={id} className="text-muted-foreground">
        {label}
      </span>
      <Switch checked={checked} disabled={disabled} onChange={onChange} labelledBy={id} />
    </span>
  );
}

/** One row of the scope list: "전체" at top level, projects indented under it. */
function ScopeButton({ icon, active, missing, title, badge, onClick, children }: { icon: ReactNode; active: boolean; missing?: boolean; title?: string; badge?: string; onClick: () => void; children: string }) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-current={active ? "page" : undefined}
      title={title}
      className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[13px] hover:bg-sidebar-accent focus-ring aria-[current=page]:bg-sidebar-accent aria-[current=page]:font-medium"
    >
      {icon}
      <span className={`min-w-0 flex-1 truncate ${missing ? "text-muted-foreground line-through" : ""}`}>{children}</span>
      {badge && <span className="shrink-0 text-3xs text-muted-foreground">{badge}</span>}
    </button>
  );
}

/** The centered empty/loading/error states of the canvas. */
function Center({ title, text, action }: { title?: string; text: string; action?: ReactNode }) {
  return (
    <div className="absolute inset-0 grid place-items-center p-6 text-center">
      <div className="space-y-3">
        {title && <p className="text-sm font-medium">{title}</p>}
        <p className="max-w-sm text-xs leading-relaxed text-muted-foreground">{text}</p>
        {action}
      </div>
    </div>
  );
}
