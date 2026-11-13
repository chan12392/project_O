import { FolderOpen, FolderPlus, FolderSearch } from "lucide-react";
import { useState } from "react";

import { sameFolder } from "../../../../src/core/projects.ts";
import { useProjectList, type Project } from "../../data/projects.ts";
import { errorText, timeOf } from "../../lib/format.ts";
import { AddFolderForm, canPickFolder, NewProjectForm, pickAndRegister } from "./ProjectForms.tsx";

const day = new Intl.DateTimeFormat("ko-KR", { month: "numeric", day: "numeric" });

/** The time for today's activity, else the day. */
function when(iso: string): string {
  const at = new Date(iso);
  return at.toDateString() === new Date().toDateString() ? timeOf(iso) : day.format(at);
}

const KIND_WORD: Record<Project["kind"], string> = { root: "프로젝트 루트", test: "test 묶음", registered: "등록한 폴더", history: "기록" };

/**
 * "새 요청": which project's lead gets the request, or a new project first (B2).
 * Picking one opens its lead chat with the input focused.
 */
export function NewRequest({ projects, current, onPick }: { projects: Project[]; current?: string; onPick: (path: string) => void }) {
  const { list, error } = useProjectList();
  const [pickError, setPickError] = useState<string>();
  const [picking, setPicking] = useState(false);
  const ordered = [...projects.filter((p) => current && sameFolder(p.path, current)), ...projects.filter((p) => !current || !sameFolder(p.path, current))];

  const pick = async () => {
    setPicking(true);
    setPickError(undefined);
    try {
      const path = await pickAndRegister();
      if (path) onPick(path);
    } catch (e) {
      setPickError(errorText(e));
    } finally {
      setPicking(false);
    }
  };

  return (
    <section className="flex min-h-0 min-w-0 flex-1 flex-col" aria-labelledby="new-request-title">
      <header className="flex h-9 shrink-0 items-center gap-2 border-b bg-tab-bar px-4 text-xs">
        <h1 id="new-request-title" className="shrink-0 font-medium text-tab-active-foreground">
          새 요청
        </h1>
        <span className="truncate text-muted-foreground">어느 프로젝트의 총괄에게 맡길지 고르세요</span>
      </header>
      <div className="scrollbar-sleek min-h-0 flex-1 overflow-y-auto bg-chat-canvas">
        <div className="mx-auto flex w-full max-w-2xl flex-col gap-8 px-5 pt-6 pb-12">
          <section aria-labelledby="new-request-projects" className="space-y-2">
            <h2 id="new-request-projects" className="text-sm font-semibold">
              프로젝트 고르기
            </h2>
            {error && <p className="text-xs text-muted-foreground">프로젝트 목록을 읽지 못해 기록에 있는 폴더를 보여 줍니다: {error}</p>}
            {ordered.length === 0 ? (
              <p className="rounded-lg border border-dashed px-3 py-4 text-xs text-muted-foreground">아직 목록에 프로젝트가 없습니다. 아래에서 새 프로젝트를 만들거나 폴더를 등록하세요.</p>
            ) : (
              <ul className="divide-y overflow-hidden rounded-lg border bg-background">
                {ordered.map((p) => {
                  const here = current !== undefined && sameFolder(p.path, current);
                  return (
                    <li key={p.path}>
                      <button
                        type="button"
                        onClick={() => onPick(p.path)}
                        disabled={p.missing}
                        className="flex w-full items-center gap-3 px-3 py-2.5 text-left hover:bg-accent/60 focus-ring focus-visible:ring-inset disabled:cursor-not-allowed disabled:opacity-60"
                      >
                        <FolderOpen className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
                        <span className="min-w-0 flex-1">
                          <span className="flex items-center gap-1.5 text-[13px]">
                            <span className="truncate font-medium">{p.name}</span>
                            {here && <span className="shrink-0 rounded-sm bg-muted px-1 text-3xs text-muted-foreground">지금 보는 프로젝트</span>}
                            {p.missing && <span className="shrink-0 text-3xs text-destructive">폴더 없음</span>}
                          </span>
                          <span className="block truncate font-mono text-2xs text-muted-foreground">{p.path}</span>
                        </span>
                        <span className="shrink-0 text-right text-2xs text-muted-foreground">
                          {KIND_WORD[p.kind]}
                          {p.lastAt && <span className="block tabular-nums">{when(p.lastAt)}</span>}
                        </span>
                      </button>
                    </li>
                  );
                })}
              </ul>
            )}
          </section>

          <section aria-labelledby="new-request-new" className="space-y-2">
            <h2 id="new-request-new" className="flex items-center gap-1.5 text-sm font-semibold">
              <FolderPlus className="size-4 text-muted-foreground" aria-hidden="true" />새 프로젝트 만들기
            </h2>
            <NewProjectForm onDone={onPick} />
          </section>

          <section aria-labelledby="new-request-folder" className="space-y-2">
            <h2 id="new-request-folder" className="flex items-center gap-1.5 text-sm font-semibold">
              <FolderSearch className="size-4 text-muted-foreground" aria-hidden="true" />
              다른 폴더 등록
            </h2>
            <p className="text-xs text-muted-foreground">
              루트 밖에 있는 저장소를 목록에 올립니다(Orca의 프로젝트 추가와 같음).
              {list && (
                <>
                  {" "}
                  루트 <span className="font-mono break-all">{list.root}</span> 안의 폴더는 등록하지 않아도 보입니다.
                </>
              )}
            </p>
            {canPickFolder() ? (
              <div className="space-y-1.5">
                <button
                  type="button"
                  onClick={() => void pick()}
                  disabled={picking}
                  className="flex h-8 items-center gap-1.5 rounded-md border px-3 text-xs hover:bg-accent focus-ring disabled:cursor-not-allowed disabled:opacity-50"
                >
                  <FolderSearch className="size-3.5" aria-hidden="true" />
                  {picking ? "폴더를 고르는 중…" : "폴더 고르기…"}
                </button>
                {pickError && (
                  <p role="alert" className="text-2xs text-destructive">
                    {pickError}
                  </p>
                )}
              </div>
            ) : (
              <AddFolderForm onDone={onPick} />
            )}
          </section>
        </div>
      </div>
    </section>
  );
}
