import { BookOpen, RefreshCw, Wrench } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { useReducedMotion, useSettings } from "../../data/settings.ts";
import { ICON_BUTTON } from "../../lib/format.ts";
import type { Backend, DreamChange, DreamGraph, GraphNode, GraphSettings } from "../../platform/backend/index.ts";
import { BUTTON_SECONDARY } from "../../ui/controls.tsx";
import { Graph3D } from "../graph/Graph3D.tsx";
import { GraphLegend } from "../graph/GraphLegend.tsx";
import { loadGraphLibs } from "../graph/graphLib.ts";
import { NodeDetail } from "../graph/NodeDetail.tsx";
import { DreamChanges } from "./DreamChanges.tsx";

const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e));

const darkQuery = () => window.matchMedia("(prefers-color-scheme: dark)").matches;

/** The node panel's words here: the nodes are documents and what glows is recent work. */
const WORDS = { kind: { root: "Dreaming", area: "묶음", file: "문서" }, glow: { active: "하루 안에 쌓인 것", path: "그것이 이어진 곳" } };

/**
 * The Dreaming tab (ADR-0041): everything the Wiki lists as one network — the materials
 * people put in and every finished card, success or failure. Records are wired by what
 * they share (a run, a follow-up, a changed file) and texts by what they name, so what
 * this PC has learned reads as a shape; the last day's work glows with what it built on.
 * Local — no LLM names the links.
 */
export function DreamView({ api, onOpenDoc }: {
  api: Backend;
  /** Opens a document in the Wiki tab. */
  onOpenDoc: (path: string) => void;
}) {
  const [state, setState] = useState<{ graph?: DreamGraph; error?: string }>();
  const [loading, setLoading] = useState(false);
  const [settings, setSettings] = useState<GraphSettings>();
  const [detail, setDetail] = useState<GraphNode>();
  const [reload, setReload] = useState(0);
  const [changes, setChanges] = useState<DreamChange[]>([]);
  const [changesOpen, setChangesOpen] = useState(false);
  const [changesReload, setChangesReload] = useState(0);
  const [changesError, setChangesError] = useState<string>();
  const reducedMotion = useReducedMotion();
  const { theme } = useSettings();
  const dark = theme === "dark" || (theme === "system" && darkQuery());

  // The 3D chunks start downloading with the data request, not after it answers.
  useEffect(() => {
    void loadGraphLibs();
  }, []);

  useEffect(() => {
    let live = true;
    // The effect switches are the Graph tab's: one setting for both canvases.
    api.graphSettings().then((s) => live && setSettings(s), () => {});
    return () => {
      live = false;
    };
  }, [api]);

  useEffect(() => {
    let live = true;
    setLoading(true);
    setDetail(undefined);
    api.dreamGraph().then(
      (graph) => live && setState({ graph }),
      // A failed refresh keeps the graph already on screen.
      (e) => live && setState((s) => ({ graph: s?.graph, error: errorText(e) })),
    ).finally(() => live && setLoading(false));
    return () => {
      live = false;
    };
  }, [api, reload]);

  // What Dreaming changed is read with the graph and again after one is taken back.
  useEffect(() => {
    let live = true;
    api.dreamChanges().then(
      (list) => {
        if (!live) return;
        setChanges(list);
        setChangesError(undefined);
      },
      // The list on screen stays; the panel says it could not be read again.
      (e) => live && setChangesError(errorText(e)),
    );
    return () => {
      live = false;
    };
  }, [api, reload, changesReload]);

  const applied = changes.filter((c) => c.state === "applied").length;
  const graph = state?.graph;
  const view = useMemo(() => (graph ? { scope: "project", project: "dreaming", nodes: graph.nodes, links: graph.links } : undefined), [graph]);
  const count = (top: string) => graph?.nodes.filter((n) => n.kind === "file" && n.path?.startsWith(`${top}/`)).length ?? 0;
  const documents = graph?.nodes.filter((n) => n.kind === "file").length ?? 0;
  const references = graph?.links.filter((l) => l.kind === "references").length ?? 0;
  const empty = graph !== undefined && documents === 0;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex h-9 shrink-0 items-center gap-3 border-b bg-tab-bar px-3 text-xs">
        <span className="font-medium">Dreaming</span>
        {graph && !empty && (
          <span className="text-muted-foreground">
            배운 것 {count("배운 것")} · 외부 자료 {count("외부 자료")} · 성공 {count("작업 성공")} · 실패 {count("작업 실패")} · 도구 오류 {count("도구 오류")} · 연결 {references}
          </span>
        )}
        <span className="ml-auto flex items-center gap-1.5">
          <button type="button" onClick={() => setChangesOpen((open) => !open)} aria-pressed={changesOpen} className="flex h-7 items-center gap-1.5 rounded-md px-2 text-muted-foreground hover:bg-accent hover:text-foreground focus-ring aria-pressed:bg-accent aria-pressed:text-foreground">
            <Wrench className="size-3.5" aria-hidden="true" />
            바꾼 것 {applied}
          </button>
          {graph?.truncated && <span className="text-2xs text-muted-foreground">문서가 많아 일부만 표시합니다</span>}
          <button type="button" onClick={() => setReload((n) => n + 1)} disabled={loading} aria-label="새로고침" title="새로고침" className={ICON_BUTTON}>
            <RefreshCw className={`size-3.5 ${loading ? "animate-spin" : ""}`} aria-hidden="true" />
          </button>
        </span>
      </header>
      <div className="relative min-h-0 flex-1 bg-chat-canvas">
        {view && !empty && (
          <Graph3D
            view={view}
            loading={false}
            effects={settings?.effects ?? true}
            particles={settings?.particles ?? true}
            reducedMotion={reducedMotion}
            // Colored by the top folder: materials, successes and failures each keep one hue.
            colorBy="area"
            dark={dark}
            onNodeClick={setDetail}
            label="Dreaming 그래프"
            className="absolute inset-0"
          />
        )}
        {!graph && loading && <Center role="status" text="쌓인 것을 엮는 중…" />}
        {!graph && !loading && state?.error && <Center role="alert" text={`Dreaming 그래프를 만들지 못했습니다: ${state.error}`} />}
        {empty && <Center text="아직 쌓인 것이 없습니다. Wiki 탭에서 외부 자료를 넣거나 카드를 실행하면, 자료와 작업 성공·실패 기록이 여기에서 서로 이어집니다." />}
        {graph && !loading && state?.error && (
          <p role="alert" className="absolute inset-x-0 top-2 z-10 mx-auto w-fit max-w-[80%] rounded-md border border-destructive/40 bg-card px-3 py-1.5 text-xs text-destructive shadow">
            Dreaming 그래프를 새로 만들지 못했습니다: {state.error}
          </p>
        )}
        {graph && !empty && <GraphLegend view={graph} colorBy="area" dark={dark} />}
        {changesOpen && <DreamChanges api={api} changes={changes} loadError={changesError} onChanged={() => setChangesReload((n) => n + 1)} onClose={() => setChangesOpen(false)} />}
        {detail && (
          <NodeDetail node={detail} words={WORDS} onClose={() => setDetail(undefined)}>
            {detail.kind === "file" && detail.path && (
              <button type="button" onClick={() => onOpenDoc(detail.path!)} className={`${BUTTON_SECONDARY} mt-3 w-full justify-center`}>
                <BookOpen className="size-3.5" aria-hidden="true" />
                Wiki에서 열기
              </button>
            )}
          </NodeDetail>
        )}
      </div>
    </div>
  );
}

function Center({ text, role }: { text: string; role?: "status" | "alert" }) {
  return (
    <div role={role} className="absolute inset-0 grid place-items-center p-6 text-center">
      <p className="max-w-sm text-xs leading-relaxed text-muted-foreground">{text}</p>
    </div>
  );
}
