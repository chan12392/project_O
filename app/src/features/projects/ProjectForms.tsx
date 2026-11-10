import { useId, useRef, useState, type FormEvent, type ReactNode } from "react";

import { sameFolder } from "../../../../src/core/projects.ts";
import { changeProjects, projectsApi, useProjectList } from "../../data/projects.ts";
import { errorText } from "../../lib/format.ts";

/**
 * The two ways to put a project on the list (ADR-0009): a new empty folder in the
 * root, or a folder elsewhere registered by path. The sidebar draws them compact,
 * the new-request screen roomy; both ask before making the root and show the
 * machine's reason when it refuses.
 */
interface FormProps {
  compact?: boolean;
  /** The listed folder, once it is on the list. */
  onDone: (path: string) => void;
  onCancel?: () => void;
}

const INPUT = "h-7 w-full min-w-0 rounded-md border bg-background px-2 text-xs focus-ring";
const PRIMARY = "flex h-7 shrink-0 items-center rounded-md bg-primary px-2.5 text-xs font-medium whitespace-nowrap text-primary-foreground hover:bg-primary/90 focus-ring disabled:cursor-not-allowed disabled:opacity-50";
const SECONDARY = "flex h-7 shrink-0 items-center rounded-md border px-2.5 text-xs whitespace-nowrap hover:bg-accent focus-ring disabled:cursor-not-allowed disabled:opacity-50";

function Shell({ compact, label, id, hint, error, children, onSubmit }: { compact?: boolean; label: string; id: string; hint?: ReactNode; error?: string; children: ReactNode; onSubmit: (e: FormEvent<HTMLFormElement>) => void }) {
  return (
    <form className={compact ? "px-3 pb-2" : "space-y-1.5"} onSubmit={onSubmit} noValidate>
      <label htmlFor={id} className={compact ? "mb-1 block text-2xs text-muted-foreground" : "block text-[13px] font-medium"}>
        {label}
      </label>
      {children}
      {hint && <p className={`text-2xs text-muted-foreground ${compact ? "mt-1" : ""}`}>{hint}</p>}
      {error && (
        <p role="alert" className={`text-2xs text-destructive ${compact ? "mt-1" : ""}`}>
          {error}
        </p>
      )}
    </form>
  );
}

/** "새 프로젝트": a folder name, made in the root. A missing root is made only after the person says so. */
export function NewProjectForm({ compact, onDone, onCancel }: FormProps) {
  const id = useId();
  const { list } = useProjectList();
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const [askRoot, setAskRoot] = useState(false);
  const input = useRef<HTMLInputElement>(null);

  const create = async (makeRoot: boolean) => {
    setBusy(true);
    setError(undefined);
    try {
      if (makeRoot) await changeProjects((api) => api.projectsRoot({ create: true }));
      const made = await changeProjects((api) => api.projectsNew(name.trim()));
      setName("");
      setAskRoot(false);
      onDone(made.created);
    } catch (e) {
      setError(errorText(e));
      requestAnimationFrame(() => input.current?.focus());
    } finally {
      setBusy(false);
    }
  };
  const submit = (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    if (busy) return;
    if (!name.trim()) return setError("프로젝트 이름을 적어 주세요");
    if (list && !list.rootExists) return setAskRoot(true);
    void create(false);
  };

  return (
    <Shell
      compact={compact}
      id={id}
      label={compact ? "새 프로젝트 이름 (Enter 만들기, Esc 취소)" : "새 프로젝트 이름"}
      error={error}
      hint={!compact && list ? <>프로젝트 루트 <span className="font-mono break-all">{list.root}</span> 안에 빈 폴더를 만듭니다.</> : undefined}
      onSubmit={submit}
    >
      <div className="flex items-center gap-1.5">
        <input
          ref={input}
          id={id}
          value={name}
          onChange={(e) => {
            setName(e.target.value);
            setError(undefined);
          }}
          autoFocus={compact}
          maxLength={64}
          placeholder={compact ? undefined : "예: 가계부-앱"}
          aria-invalid={error ? true : undefined}
          onKeyDown={(e) => e.key === "Escape" && onCancel?.()}
          className={INPUT}
        />
        {!compact && (
          <button type="submit" disabled={busy} className={PRIMARY}>
            {busy ? "만드는 중…" : "만들기"}
          </button>
        )}
      </div>
      {askRoot && list && (
        <div role="alertdialog" aria-label="프로젝트 루트 폴더 만들기" className="mt-1.5 rounded-md border bg-muted/40 p-2 text-2xs">
          <p>
            프로젝트 루트 폴더 <span className="font-mono break-all">{list.root}</span>가 아직 없습니다. 만들고 그 안에 “{name.trim()}”을(를) 만들까요?
          </p>
          <div className="mt-1.5 flex gap-1.5">
            <button type="button" autoFocus disabled={busy} onClick={() => void create(true)} className={PRIMARY}>
              {busy ? "만드는 중…" : "루트 만들고 진행"}
            </button>
            <button type="button" disabled={busy} onClick={() => setAskRoot(false)} className={SECONDARY}>
              취소
            </button>
          </div>
        </div>
      )}
    </Shell>
  );
}

/** Registers a folder outside the root by its path (the browser preview has no folder picker). */
export function AddFolderForm({ compact, onDone, onCancel }: FormProps) {
  const id = useId();
  const [path, setPath] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();

  const submit = async (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    if (busy) return;
    if (!path.trim()) return setError("폴더 경로를 적어 주세요");
    setBusy(true);
    setError(undefined);
    try {
      const listed = await registerFolder(path.trim());
      setPath("");
      onDone(listed);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Shell
      compact={compact}
      id={id}
      label={compact ? "등록할 폴더 경로 (Enter 등록, Esc 취소)" : "폴더 경로"}
      error={error}
      hint={compact ? undefined : "목록에서 빼도 폴더와 기록은 지워지지 않습니다. 홈 폴더 자체와 O 상태 폴더는 등록할 수 없습니다."}
      onSubmit={(e) => void submit(e)}
    >
      <div className="flex items-center gap-1.5">
        <input
          id={id}
          value={path}
          onChange={(e) => {
            setPath(e.target.value);
            setError(undefined);
          }}
          autoFocus={compact}
          spellCheck={false}
          placeholder={"C:\\work\\my-app"}
          aria-invalid={error ? true : undefined}
          onKeyDown={(e) => e.key === "Escape" && onCancel?.()}
          className={`${INPUT} font-mono`}
        />
        {!compact && (
          <button type="submit" disabled={busy} className={PRIMARY}>
            {busy ? "등록하는 중…" : "등록"}
          </button>
        )}
      </div>
    </Shell>
  );
}

/** Registers one folder; resolves to the path as the list spells it. */
export async function registerFolder(path: string): Promise<string> {
  const list = await changeProjects((api) => api.projectsAdd(path));
  return list.projects.find((p) => sameFolder(p.path, path))?.path ?? path;
}

/** Whether this machine has a native folder picker (the desktop app). */
export function canPickFolder(): boolean {
  return Boolean(projectsApi()?.pickFolder);
}

/** The native picker, then registering what was picked; null when cancelled. */
export async function pickAndRegister(): Promise<string | null> {
  const picked = await projectsApi()?.pickFolder?.();
  return picked ? registerFolder(picked) : null;
}
