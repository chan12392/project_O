import { X } from "lucide-react";

import type { GraphNode } from "../../platform/backend/types.ts";
import { countText } from "./graphText.ts";

const KIND_WORD: Record<GraphNode["kind"], string> = { root: "전체", project: "프로젝트", area: "영역", file: "파일" };
const GLOW_WORD: Record<NonNullable<GraphNode["glow"]>, string> = { active: "지금 바뀐 곳", path: "그 영향 경로" };

/** The picked node's details, floating over the canvas's top right. */
export function NodeDetail({ node, onClose }: { node: GraphNode; onClose: () => void }) {
  return (
    <aside className="absolute right-2 top-2 z-10 w-64 rounded-lg border bg-card p-3 text-xs shadow-lg" aria-label="노드 정보">
      <header className="flex items-start gap-2">
        <span className="min-w-0 flex-1 font-medium">{node.label}</span>
        <button type="button" onClick={onClose} aria-label="닫기" className="shrink-0 rounded focus-ring">
          <X className="size-3.5" aria-hidden="true" />
        </button>
      </header>
      <dl className="mt-2 space-y-1 text-muted-foreground">
        <div className="flex gap-2"><dt className="w-12 shrink-0">종류</dt><dd>{KIND_WORD[node.kind]}</dd></div>
        {node.path && <div className="flex gap-2"><dt className="w-12 shrink-0">경로</dt><dd className="min-w-0 font-mono text-2xs break-all">{node.path}</dd></div>}
        {node.project && <div className="flex gap-2"><dt className="w-12 shrink-0">프로젝트</dt><dd className="min-w-0 font-mono text-2xs break-all">{node.project}</dd></div>}
        {node.glow && <div className="flex gap-2"><dt className="w-12 shrink-0">상태</dt><dd className="text-agent-working">{GLOW_WORD[node.glow]}</dd></div>}
        {node.error && <div className="flex gap-2"><dt className="w-12 shrink-0">오류</dt><dd className="min-w-0 text-destructive break-all">{node.error}</dd></div>}
        {node.kind === "project" && node.indexed === false && <div className="flex gap-2"><dt className="w-12 shrink-0">인덱스</dt><dd>없음</dd></div>}
        <div className="flex gap-2"><dt className="w-12 shrink-0">크기</dt><dd>{countText(node.size)}</dd></div>
        {node.summary && <div className="flex gap-2"><dt className="w-12 shrink-0">요약</dt><dd className="min-w-0">{node.summary}</dd></div>}
      </dl>
    </aside>
  );
}
