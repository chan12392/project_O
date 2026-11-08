import { Boxes, RefreshCw, Sparkles } from "lucide-react";

import type { GraphColorBy, GraphSettings, GraphView } from "../../platform/backend/types.ts";
import { ICON_BUTTON } from "../../lib/format.ts";
import { BUTTON_PRIMARY, BUTTON_SECONDARY, Segmented, Switch } from "../../ui/controls.tsx";

/**
 * The Graph tab's header (ADR-0033): the 효과/흐름 switches and 색 기준 on the left, then the
 * actions — 새로고침 always, 인덱스 만들기 and Deep for a project scope with grafly.
 */
export function GraphToolbar({ settings, reducedMotion, onSettings, view, loading, onRefresh, project, notInstalled, busy, onBuild, onDeep }: {
  settings?: GraphSettings;
  reducedMotion: boolean;
  onSettings: (change: Partial<GraphSettings>) => void;
  view?: GraphView;
  loading: boolean;
  onRefresh: () => void;
  /** Set on a project scope; the build/Deep actions only make sense there. */
  project?: string;
  notInstalled: boolean;
  busy?: "build" | "deep";
  onBuild: () => void;
  onDeep: () => void;
}) {
  return (
    <header className="flex h-9 shrink-0 items-center gap-3 border-b bg-tab-bar px-3 text-xs">
      <span className="font-medium">Graph</span>
      <ToolSwitch id="graph-effects" label="효과" checked={settings?.effects ?? true} disabled={!settings} onChange={(effects) => onSettings({ effects })} />
      <ToolSwitch id="graph-particles" label="흐름" checked={settings?.particles ?? true} disabled={!settings || reducedMotion} onChange={(particles) => onSettings({ particles })} />
      {reducedMotion && <span className="text-2xs text-muted-foreground">동작 줄이기 켜짐 — 흐름 끔</span>}
      <Segmented<GraphColorBy>
        label="색 기준"
        value={settings?.colorBy ?? "language"}
        disabled={!settings}
        onChange={(colorBy) => onSettings({ colorBy })}
        options={[
          { value: "language", label: "언어별" },
          { value: "area", label: "영역별" },
        ]}
      />
      <span className="ml-auto flex items-center gap-1.5">
        {view?.truncated && <span className="text-2xs text-muted-foreground">노드가 많아 일부만 표시합니다</span>}
        <button type="button" onClick={onRefresh} disabled={loading} aria-label="새로고침" title="새로고침" className={ICON_BUTTON}>
          <RefreshCw className={`size-3.5 ${loading ? "animate-spin" : ""}`} aria-hidden="true" />
        </button>
        {project && !notInstalled && (
          <button
            type="button"
            onClick={onBuild}
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
