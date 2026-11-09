import { Boxes } from "lucide-react";
import type { ReactNode } from "react";

import type { GraphView } from "../../platform/backend/types.ts";
import { BUTTON_PRIMARY, BUTTON_SECONDARY } from "../../ui/controls.tsx";
import { deepStampText } from "./graphText.ts";

/**
 * What covers the canvas while there is no graph to show, and the small status
 * chips over it while there is (ADR-0033). A same-scope reload keeps its previous
 * view, so `Graph3D` stays mounted — camera and library intact — and only a chip
 * reports the refresh or a failed one.
 */
export function CanvasState({ view, loading, error, project, busy, notInstalled, onRetry, onBuild }: {
  view?: GraphView;
  loading: boolean;
  error?: string;
  /** A project scope's folder; the empty state offers 인덱스 만들기 for it. */
  project?: string;
  busy?: "build" | "deep";
  notInstalled: boolean;
  onRetry: () => void;
  onBuild: () => void;
}) {
  const graphUp = view !== undefined && !notInstalled && view.nodes.length > 0;
  /** With no graph on screen a load failure takes the whole canvas, as before. */
  const errorCenter = error !== undefined && !loading && !graphUp;
  return (
    <>
      {!view && loading && <Center role="status" text="그래프를 불러오는 중…" />}
      {errorCenter && (
        <Center
          role="alert"
          text={`그래프를 불러오지 못했습니다: ${error}`}
          action={
            <button type="button" onClick={onRetry} className={BUTTON_SECONDARY}>
              다시 시도
            </button>
          }
        />
      )}
      {view && !errorCenter && notInstalled && (
        <Center
          title="grafly가 설치되어 있지 않습니다"
          text="코드 그래프는 grafly가 만듭니다. O는 grafly를 내려받거나 설치하지 않습니다. grafly를 설치한 뒤 프로젝트에서 인덱스를 만들어 주세요."
        />
      )}
      {view && !errorCenter && !notInstalled && view.nodes.length === 0 && (
        <Center
          title={project ? "인덱스가 없습니다" : "인덱스된 프로젝트가 없습니다"}
          text={project ? "이 프로젝트의 코드 그래프를 보려면 먼저 인덱스를 만드세요." : "프로젝트에서 인덱스를 만들면 전체 그래프에 나타납니다."}
          action={
            project ? (
              <button type="button" onClick={onBuild} disabled={busy !== undefined} className={BUTTON_PRIMARY}>
                <Boxes className="size-3.5" aria-hidden="true" />
                인덱스 만들기
              </button>
            ) : undefined
          }
        />
      )}
      {graphUp && loading && !busy && <Chip>그래프를 새로고침하는 중…</Chip>}
      {graphUp && !loading && !busy && error && (
        <p role="alert" className="absolute inset-x-0 top-2 z-10 mx-auto flex w-fit max-w-[80%] items-center gap-2 rounded-md border border-destructive/40 bg-card px-3 py-1.5 text-xs text-destructive shadow">
          <span className="min-w-0">그래프를 불러오지 못했습니다: {error}</span>
          <button type="button" onClick={onRetry} className="shrink-0 rounded underline underline-offset-2 focus-ring">
            다시 시도
          </button>
        </p>
      )}
      {busy === "deep" && (
        <div role="status" className="absolute inset-0 z-10 grid place-items-center bg-background/60">
          <p className="rounded-lg border bg-card px-4 py-3 text-sm shadow-lg">
            Deep으로 영역 이름과 요약을 만드는 중…
            <span className="mt-1 block text-xs text-muted-foreground">몇 분 걸릴 수 있습니다.</span>
          </p>
        </div>
      )}
      {busy === "build" && <Chip>인덱스를 만드는 중…</Chip>}
      {graphUp && view?.deep && !busy && (
        <p className="absolute bottom-2 left-2 rounded-md bg-card/80 px-2 py-1 text-2xs text-muted-foreground">{deepStampText(view.deep)}</p>
      )}
    </>
  );
}

/** A small status line floating over the canvas's top edge. */
function Chip({ children }: { children: ReactNode }) {
  return (
    <p role="status" className="absolute inset-x-0 top-2 z-10 mx-auto w-fit rounded-md border bg-card px-3 py-1.5 text-xs shadow">
      {children}
    </p>
  );
}

/** The centered empty/loading/error states of the canvas. */
function Center({ title, text, action, role }: { title?: string; text: string; action?: ReactNode; role?: "status" | "alert" }) {
  return (
    <div role={role} className="absolute inset-0 grid place-items-center p-6 text-center">
      <div className="space-y-3">
        {title && <p className="text-sm font-medium">{title}</p>}
        <p className="max-w-sm text-xs leading-relaxed text-muted-foreground">{text}</p>
        {action}
      </div>
    </div>
  );
}
