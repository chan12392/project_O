import { FolderTree } from "lucide-react";
import { useId, useState, type FormEvent } from "react";

import { changeProjects, projectsApi, useProjectList } from "../../../data/projects.ts";
import { canPickFolder } from "../../projects/ProjectForms.tsx";
import { Path, Section, type SettingsSectionDef } from "../Section.tsx";

export const PROJECTS_SECTION = { id: "projects", title: "프로젝트", icon: FolderTree, render: () => <ProjectsSection /> } as const satisfies SettingsSectionDef;

const BUTTON = "flex h-7 items-center rounded-md border px-2.5 text-xs whitespace-nowrap hover:bg-accent focus-ring disabled:cursor-not-allowed disabled:opacity-50";

/** Settings → 프로젝트 (ADR-0009): the root whose folders are projects, and the folders registered outside it. */
function ProjectsSection() {
  const { list, error, pending } = useProjectList();
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string>();
  const [editing, setEditing] = useState(false);
  const [askCreate, setAskCreate] = useState(false);
  const fieldId = useId();

  const run = async (change: () => Promise<unknown>) => {
    setBusy(true);
    setProblem(undefined);
    try {
      await change();
      return true;
    } catch (e) {
      setProblem(e instanceof Error ? e.message : String(e));
      return false;
    } finally {
      setBusy(false);
    }
  };
  const moveRoot = (path: string) => run(() => changeProjects((api) => api.projectsRoot({ path })));
  const pickRoot = async () => {
    const picked = await projectsApi()?.pickFolder?.();
    if (picked) await moveRoot(picked);
  };
  const submitRoot = async (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    const path = new FormData(e.currentTarget).get("root")?.toString().trim();
    if (path && (await moveRoot(path))) setEditing(false);
  };
  const registered = list?.projects.filter((p) => p.kind === "registered") ?? [];
  const counts = { root: list?.projects.filter((p) => p.kind === "root").length ?? 0, test: list?.projects.filter((p) => p.kind === "test").length ?? 0 };

  return (
    <Section title="프로젝트" description="왼쪽 목록에 뜨는 프로젝트를 정합니다. 프로젝트 루트의 하위 폴더는 저절로 뜨고, 루트 밖의 폴더는 등록해서 올립니다. 목록에서 빼도 폴더와 기록은 지워지지 않습니다.">
      {pending && <p className="text-xs text-muted-foreground">목록을 읽는 중…</p>}
      {error && !list && <p role="alert" className="text-xs text-destructive">프로젝트 목록을 읽지 못했습니다: {error}</p>}
      {list && (
        <>
          <dl className="divide-y">
            <div className="grid grid-cols-[10rem_minmax(0,1fr)] gap-4 py-3 text-[13px]">
              <dt className="text-muted-foreground">프로젝트 루트</dt>
              <dd className="min-w-0 space-y-2">
                <p>
                  <Path path={list.root} /> <span className="text-2xs text-muted-foreground">{list.defaultRoot ? "(기본)" : "(직접 고름)"}</span>
                </p>
                <p className="text-xs text-muted-foreground">
                  {list.rootExists
                    ? `하위 폴더 ${counts.root}개가 프로젝트입니다. 그 안의 test 폴더(${counts.test}개)는 접힌 "test" 묶음으로 보입니다. 점(.)으로 시작하는 폴더는 뺍니다.`
                    : "아직 없는 폴더입니다. 만들면 그 안에 새 프로젝트를 만들 수 있습니다."}
                </p>
                <div className="flex flex-wrap gap-1.5">
                  {!list.rootExists &&
                    (askCreate ? (
                      <span role="alertdialog" aria-label="프로젝트 루트 만들기" className="flex flex-wrap items-center gap-1.5 text-xs">
                        이 폴더를 만들까요?
                        <button type="button" autoFocus disabled={busy} onClick={() => void run(() => changeProjects((api) => api.projectsRoot({ create: true }))).then(() => setAskCreate(false))} className={BUTTON}>
                          만들기
                        </button>
                        <button type="button" onClick={() => setAskCreate(false)} className={BUTTON}>
                          취소
                        </button>
                      </span>
                    ) : (
                      <button type="button" disabled={busy} onClick={() => setAskCreate(true)} className={BUTTON}>
                        루트 폴더 만들기…
                      </button>
                    ))}
                  {canPickFolder() ? (
                    <button type="button" disabled={busy} onClick={() => void pickRoot()} className={BUTTON}>
                      다른 폴더로 바꾸기…
                    </button>
                  ) : (
                    !editing && (
                      <button type="button" disabled={busy} onClick={() => setEditing(true)} className={BUTTON}>
                        다른 폴더로 바꾸기
                      </button>
                    )
                  )}
                  {!list.defaultRoot && (
                    <button type="button" disabled={busy} onClick={() => void run(() => changeProjects((api) => api.projectsRoot({ reset: true })))} className={BUTTON}>
                      기본 폴더로 되돌리기
                    </button>
                  )}
                </div>
                {editing && (
                  <form onSubmit={(e) => void submitRoot(e)} className="flex flex-wrap items-center gap-1.5">
                    <label htmlFor={fieldId} className="sr-only">
                      새 프로젝트 루트 경로
                    </label>
                    <input id={fieldId} name="root" defaultValue={list.root} autoFocus spellCheck={false} className="h-7 min-w-0 flex-1 rounded-md border bg-background px-2 font-mono text-xs focus-ring" />
                    <button type="submit" disabled={busy} className={BUTTON}>
                      바꾸기
                    </button>
                    <button type="button" onClick={() => setEditing(false)} className={BUTTON}>
                      취소
                    </button>
                  </form>
                )}
              </dd>
            </div>
            <div className="grid grid-cols-[10rem_minmax(0,1fr)] gap-4 py-3 text-[13px]">
              <dt className="text-muted-foreground">등록한 폴더</dt>
              <dd className="min-w-0">
                {registered.length === 0 ? (
                  <p className="text-xs text-muted-foreground">없습니다. 왼쪽 목록 머리글의 + 로 루트 밖의 폴더를 등록합니다.</p>
                ) : (
                  <ul className="divide-y rounded-md border">
                    {registered.map((p) => (
                      <li key={p.path} className="flex items-center gap-2 px-2.5 py-2">
                        <span className="min-w-0 flex-1">
                          <span className="block text-xs font-medium">
                            {p.name}
                            {p.missing && <span className="ml-1.5 text-3xs font-normal text-destructive">폴더 없음</span>}
                          </span>
                          <Path path={p.path} />
                        </span>
                        <button type="button" disabled={busy} onClick={() => void run(() => changeProjects((api) => api.projectsRemove(p.path)))} className={BUTTON}>
                          목록에서 빼기
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
              </dd>
            </div>
          </dl>
          {problem && (
            <p role="alert" className="text-xs text-destructive">
              {problem}
            </p>
          )}
          <p className="text-xs text-muted-foreground">
            목록에 없는 폴더의 실행·총괄 대화·Claude Code 세션은 지우지 않고 화면에서만 숨깁니다. 홈 폴더 자체, O 상태 폴더, 보호 브랜치 거울은 프로젝트로 쓸 수 없습니다.
          </p>
        </>
      )}
    </Section>
  );
}
