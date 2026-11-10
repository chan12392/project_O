import { useState, type FormEvent } from "react";

import type { Project } from "../../data/projects.ts";
import type { Backend, DreamDoc, DreamInput } from "../../platform/backend/index.ts";
import { BUTTON_PRIMARY, BUTTON_SECONDARY } from "../../ui/controls.tsx";
import { titleFromFile } from "./wikiTree.ts";

const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e));

type Mode = "text" | "file" | "url";

const MODES: { id: Mode; label: string }[] = [
  { id: "text", label: "글" },
  { id: "file", label: "파일" },
  { id: "url", label: "웹 주소" },
];

/** What `o dream add` keeps of one material. */
const MAX_TEXT_BYTES = 1024 * 1024;
const FILE_TYPES = ".md,.markdown,.txt,.log,.json,.csv,.yaml,.yml,.toml,.html,.htm";
const FIELD = "h-8 w-full min-w-0 rounded-md border bg-background px-2 text-xs focus-ring";

/**
 * Puts an outside material into Dreaming (ADR-0041): text typed or pasted, a text file's
 * content, or a web page fetched once by its address. A file is read here in the window —
 * only its text and its name go on, never its location.
 */
export function AddMaterial({ api, projects, onAdded, onCancel }: { api: Backend; projects: Project[]; onAdded: (doc: DreamDoc) => void; onCancel: () => void }) {
  const [mode, setMode] = useState<Mode>("text");
  const [title, setTitle] = useState("");
  const [text, setText] = useState("");
  const [file, setFile] = useState<{ name: string; text: string }>();
  const [url, setUrl] = useState("");
  const [project, setProject] = useState("");
  const [tags, setTags] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();

  const pickFile = async (picked: File | undefined) => {
    setError(undefined);
    setFile(undefined);
    if (!picked) return;
    if (picked.size > MAX_TEXT_BYTES) return setError("파일이 너무 큽니다 (1 MiB까지).");
    let content: string;
    try {
      content = await picked.text();
    } catch {
      return setError("파일을 읽지 못했습니다.");
    }
    if (content.includes("\0")) return setError("글자로 읽을 수 없는 파일입니다.");
    // A file in another encoding (CP949 and the like) would be stored as broken characters.
    if (content.includes("\uFFFD")) return setError("UTF-8로 저장된 글 파일만 넣을 수 있습니다. 파일을 UTF-8로 다시 저장해 주세요.");
    setFile({ name: picked.name, text: content });
    setTitle((t) => t || titleFromFile(picked.name));
  };

  const input = (): DreamInput | string => {
    const common = { title: title.trim() || undefined, project: project || undefined, tags: tags.trim() || undefined };
    if (mode === "url") return /^https?:\/\/\S+$/i.test(url.trim()) ? { kind: "url", url: url.trim(), ...common } : "http:// 또는 https://로 시작하는 주소를 넣으세요.";
    if (mode === "file") return file ? { kind: "text", text: file.text, source: file.name, ...common } : "넣을 파일을 고르세요.";
    return text.trim() ? { kind: "text", text, ...common } : "넣을 글을 적으세요.";
  };

  const submit = (e: FormEvent) => {
    e.preventDefault();
    const made = input();
    if (typeof made === "string") return setError(made);
    setBusy(true);
    setError(undefined);
    api.dreamAdd(made).then(onAdded, (reason) => setError(errorText(reason))).finally(() => setBusy(false));
  };

  return (
    <form onSubmit={submit} className="mx-auto flex max-w-2xl flex-col gap-3 px-6 py-5 text-xs" aria-label="외부 자료 넣기">
      <h2 className="text-sm font-medium">외부 자료 넣기</h2>
      <div role="radiogroup" aria-label="자료 형태" className="flex w-fit gap-0.5 rounded-md border bg-background p-0.5">
        {MODES.map(({ id, label }) => (
          <button key={id} type="button" role="radio" aria-checked={mode === id} onClick={() => { setMode(id); setError(undefined); }} className="h-6 rounded px-2.5 text-muted-foreground focus-ring aria-checked:bg-accent aria-checked:font-medium aria-checked:text-foreground">
            {label}
          </button>
        ))}
      </div>
      {mode === "text" && (
        <label className="flex flex-col gap-1">
          <span className="text-muted-foreground">내용 (Markdown)</span>
          <textarea value={text} onChange={(e) => setText(e.target.value)} rows={12} className="scrollbar-sleek w-full resize-y rounded-md border bg-background p-2 font-mono text-xs focus-ring" />
        </label>
      )}
      {mode === "file" && (
        <label className="flex flex-col gap-1">
          <span className="text-muted-foreground">글자로 된 파일 (1 MiB까지)</span>
          <input type="file" accept={FILE_TYPES} onChange={(e) => void pickFile(e.target.files?.[0])} className="text-xs file:mr-2 file:h-7 file:rounded-md file:border file:bg-background file:px-2 file:text-xs" />
          {file && <span className="text-2xs text-muted-foreground">{file.name} · {file.text.length.toLocaleString()}자</span>}
        </label>
      )}
      {mode === "url" && (
        <label className="flex flex-col gap-1">
          <span className="text-muted-foreground">주소</span>
          <input value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://" inputMode="url" className={FIELD} />
          <span className="text-2xs text-muted-foreground">이 PC가 주소의 글을 한 번 읽어 와 저장합니다. 나중에 페이지가 바뀌어도 따라 바뀌지 않습니다.</span>
        </label>
      )}
      <label className="flex flex-col gap-1">
        <span className="text-muted-foreground">제목 {mode !== "file" && "(비우면 내용에서 정합니다)"}</span>
        <input value={title} onChange={(e) => setTitle(e.target.value)} maxLength={120} className={FIELD} />
      </label>
      <label className="flex flex-col gap-1">
        <span className="text-muted-foreground">관련 프로젝트</span>
        <select value={project} onChange={(e) => setProject(e.target.value)} className={FIELD}>
          <option value="">없음</option>
          {projects.filter((p) => !p.missing).map((p) => (
            <option key={p.path} value={p.path}>
              {p.name}
            </option>
          ))}
        </select>
      </label>
      <label className="flex flex-col gap-1">
        <span className="text-muted-foreground">태그 (쉼표나 띄어쓰기로 나눕니다. 카드가 이 태그로 자료를 찾습니다)</span>
        <input value={tags} onChange={(e) => setTags(e.target.value)} placeholder="배포, 서명" className={FIELD} />
      </label>
      {error && <p role="alert" className="text-destructive">{error}</p>}
      <div className="flex justify-end gap-2">
        <button type="button" onClick={onCancel} disabled={busy} className={BUTTON_SECONDARY}>
          취소
        </button>
        <button type="submit" disabled={busy} className={BUTTON_PRIMARY}>
          {busy ? "넣는 중…" : "넣기"}
        </button>
      </div>
    </form>
  );
}
