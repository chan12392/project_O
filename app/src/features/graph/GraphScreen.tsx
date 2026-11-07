import { X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import type { Project } from "../../data/projects.ts";
import { useReducedMotion } from "../../data/settings.ts";
import type { Backend, GraphNode, GraphSettings } from "../../platform/backend/index.ts";
import { CanvasState } from "./CanvasState.tsx";
import { DeepConfirm } from "./DeepConfirm.tsx";
import { Graph3D } from "./Graph3D.tsx";
import { GraphToolbar } from "./GraphToolbar.tsx";
import { finite } from "./graphText.ts";
import { deepModelOf } from "./modelTier.ts";
import { NodeDetail } from "./NodeDetail.tsx";
import { ScopeList } from "./ScopeList.tsx";
import { projectOf, useGraphView, type GraphScope } from "./useGraphView.ts";

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
  const project = projectOf(scope);

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
      const result = await api.graphDeep(project, settings.deepProvider, deepModelOf(settings) || undefined);
      const { input, output } = result.usage;
      const tokens = finite(input) && finite(output) ? `, 입력 ${input.toLocaleString()}·출력 ${output.toLocaleString()} 토큰` : "";
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

  const notInstalled = view !== undefined && !view.grafly.installed;
  /** The canvas stays mounted while a same-scope reload keeps its previous view. */
  const showGraph = view !== undefined && !notInstalled && view.nodes.length > 0;

  return (
    <section className="flex min-h-0 min-w-0 flex-1 flex-col" aria-label="Graph">
      <GraphToolbar
        settings={settings}
        reducedMotion={reducedMotion}
        onSettings={(change) => void changeSettings(change)}
        view={view}
        loading={loading}
        onRefresh={() => void refresh()}
        project={project}
        notInstalled={notInstalled}
        busy={busy}
        onBuild={() => void build()}
        onDeep={onDeep}
      />
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
        <ScopeList
          projects={projects}
          scope={scope}
          current={current}
          indexed={indexed}
          onPick={(next) => {
            setDetail(undefined);
            setScope(next);
          }}
        />
        <div className="relative min-w-0 flex-1 bg-chat-canvas">
          {showGraph && view && <Graph3D view={view} effects={settings?.effects ?? true} particles={settings?.particles ?? true} reducedMotion={reducedMotion} onNodeClick={onNode} className="absolute inset-0" />}
          <CanvasState view={view} loading={loading} error={error} project={project} busy={busy} notInstalled={notInstalled} onRetry={() => void refresh()} onBuild={() => void build()} />
          {detail && <NodeDetail node={detail} onClose={() => setDetail(undefined)} />}
        </div>
      </div>
      {confirm && settings && project && (
        <DeepConfirm
          provider={settings.deepProvider}
          model={deepModelOf(settings)}
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
