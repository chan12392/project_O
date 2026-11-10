import { ChevronRight, CircleCheck, CircleX, FileText, Folder, Lightbulb, Plus, RefreshCw, Search, Trash2, TriangleAlert } from "lucide-react";
import { memo, useEffect, useMemo, useState, type MouseEvent, type ReactNode } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";

import type { Project } from "../../data/projects.ts";
import { ICON_BUTTON } from "../../lib/format.ts";
import { backend, type Backend, type DreamDoc, type DreamList, type DreamPage } from "../../platform/backend/index.ts";
import { BUTTON_SECONDARY } from "../../ui/controls.tsx";
import { safeUrl } from "../chat/markdown.ts";
import { AddMaterial } from "./AddMaterial.tsx";
import { buildTree, countDocs, fileName, filterDocs, firstDoc, KIND_WORD, resolveLink, type WikiFolder } from "./wikiTree.ts";

const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e));

const timeText = (at: string) => {
  const time = new Date(at);
  return Number.isNaN(time.getTime()) ? at : time.toLocaleString("ko-KR", { dateStyle: "medium", timeStyle: "short" });
};

/**
 * The Wiki tab (ADR-0041): what Dreaming holds, as documents. The left column lists them
 * under their folders — materials people put in, then each finished card under 작업 성공 or
 * 작업 실패 and its project; the right shows the picked one with what it is wired to.
 * Materials are added and removed here; card records come from the run history by themselves.
 */
export function WikiScreen({ api, projects, doc, onDoc }: {
  api: Backend;
  projects: Project[];
  /** The open document's path; the newest one is opened when none is picked. */
  doc?: string;
  onDoc: (path: string) => void;
}) {
  const [list, setList] = useState<{ value?: DreamList; error?: string }>();
  const [page, setPage] = useState<{ key: string; value?: DreamPage; error?: string }>();
  const [query, setQuery] = useState("");
  const [closed, setClosed] = useState<ReadonlySet<string>>(new Set());
  const [reload, setReload] = useState(0);
  const [adding, setAdding] = useState(false);

  useEffect(() => {
    let live = true;
    api.dreamList().then(
      (value) => live && setList({ value }),
      (e) => live && setList((l) => ({ value: l?.value, error: errorText(e) })),
    );
    return () => {
      live = false;
    };
  }, [api, reload]);

  const docs = list?.value?.docs;
  const open = docs && (docs.some((d) => d.path === doc) ? doc : firstDoc(docs));

  useEffect(() => {
    if (!open) return;
    let live = true;
    api.dreamRead(open).then(
      (value) => live && setPage({ key: open, value }),
      (e) => live && setPage({ key: open, error: errorText(e) }),
    );
    return () => {
      live = false;
    };
  }, [api, open, reload]);

  const shown = useMemo(() => filterDocs(docs ?? [], query), [docs, query]);
  const tree = useMemo(() => buildTree(shown), [shown]);
  const paths = useMemo(() => (docs ?? []).map((d) => d.path), [docs]);
  const current = page?.key === open ? page : undefined;

  const remove = (path: string) =>
    api.dreamRemove(path).then(() => {
      setPage(undefined);
      // Out of the list at once: reading it again before the new list lands would only fail.
      setList((l) => (l?.value ? { ...l, value: { ...l.value, docs: l.value.docs.filter((d) => d.path !== path) } } : l));
      setReload((n) => n + 1);
    });

  return (
    <div className="flex min-h-0 flex-1">
      <nav className="flex w-64 shrink-0 flex-col border-r" aria-label="문서 목록">
        <div className="flex shrink-0 items-center gap-1.5 border-b p-2">
          <button type="button" onClick={() => setAdding(true)} className={`${BUTTON_SECONDARY} h-7 flex-1 justify-center`}>
            <Plus className="size-3.5" aria-hidden="true" />
            외부 자료 넣기
          </button>
          <button type="button" onClick={() => setReload((n) => n + 1)} aria-label="새로고침" title="새로고침" className={ICON_BUTTON}>
            <RefreshCw className="size-3.5" aria-hidden="true" />
          </button>
        </div>
        <label className="flex shrink-0 items-center gap-1.5 border-b px-2 py-1.5 text-muted-foreground">
          <Search className="size-3.5 shrink-0" aria-hidden="true" />
          <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="문서 찾기" aria-label="문서 찾기" className="min-w-0 flex-1 bg-transparent text-xs text-foreground outline-none placeholder:text-muted-foreground" />
        </label>
        <div className="scrollbar-sleek min-h-0 flex-1 overflow-y-auto p-1.5">
          {!list && <Hint>문서를 찾는 중…</Hint>}
          {list?.error && <Hint alert>문서 목록을 읽지 못했습니다: {list.error}</Hint>}
          {docs && docs.length === 0 && <Hint>아직 쌓인 것이 없습니다. 외부 자료를 넣거나 카드를 실행하면 여기에 모입니다.</Hint>}
          {docs && docs.length > 0 && shown.length === 0 && <Hint>찾는 문서가 없습니다.</Hint>}
          {docs && <FolderRows folder={tree} depth={0} open={adding ? undefined : open} closed={query ? EMPTY : closed} onToggle={(path) => setClosed((s) => toggled(s, path))} onDoc={(path) => { setAdding(false); onDoc(path); }} />}
          {list?.value?.truncated && <Hint>문서가 많아 최근 것만 표시합니다.</Hint>}
        </div>
      </nav>
      <article className="scrollbar-sleek min-w-0 flex-1 overflow-y-auto bg-chat-canvas">
        {adding ? (
          <AddMaterial
            api={api}
            projects={projects}
            onCancel={() => setAdding(false)}
            onAdded={(added) => {
              setAdding(false);
              onDoc(added.path);
              setReload((n) => n + 1);
            }}
          />
        ) : (
          <>
            {list && !open && !list.error && <p className="p-6 text-sm text-muted-foreground">왼쪽 위 "외부 자료 넣기"로 첫 자료를 넣어 보세요.</p>}
            {open && !current && <p role="status" className="p-6 text-sm text-muted-foreground">문서를 여는 중…</p>}
            {current?.error && <p role="alert" className="p-6 text-sm text-destructive">문서를 읽지 못했습니다: {current.error}</p>}
            {current?.value && (
              <div className="mx-auto max-w-3xl px-6 py-5">
                <PageHead key={current.value.path} page={current.value} onRemove={remove} />
                <Document page={current.value} paths={paths} onDoc={onDoc} />
                {current.value.truncated && <p className="mt-4 text-xs text-muted-foreground">문서가 길어 앞부분만 표시했습니다.</p>}
                <LinkList title="이 문서가 이어진 문서" paths={current.value.links} docs={docs ?? []} onDoc={onDoc} />
                <LinkList title="이 문서로 이어진 문서" paths={current.value.backlinks} docs={docs ?? []} onDoc={onDoc} />
              </div>
            )}
          </>
        )}
      </article>
    </div>
  );
}

const EMPTY: ReadonlySet<string> = new Set();

function toggled(set: ReadonlySet<string>, path: string): ReadonlySet<string> {
  const next = new Set(set);
  if (!next.delete(path)) next.add(path);
  return next;
}

function Hint({ children, alert }: { children: ReactNode; alert?: boolean }) {
  return (
    <p role={alert ? "alert" : undefined} className={`px-2 py-1.5 text-2xs ${alert ? "text-destructive" : "text-muted-foreground"}`}>
      {children}
    </p>
  );
}

const ROW = "flex w-full items-center gap-1.5 rounded-md py-1 pr-2 text-left text-[13px] hover:bg-sidebar-accent focus-ring aria-[current=page]:bg-sidebar-accent aria-[current=page]:font-medium";

function DocIcon({ kind }: { kind: DreamDoc["kind"] }) {
  if (kind === "done") return <CircleCheck className="size-3.5 shrink-0 text-status-success" aria-hidden="true" />;
  if (kind === "failed") return <CircleX className="size-3.5 shrink-0 text-destructive" aria-hidden="true" />;
  if (kind === "tool") return <TriangleAlert className="size-3.5 shrink-0 text-agent-question-text" aria-hidden="true" />;
  if (kind === "note") return <Lightbulb className="size-3.5 shrink-0 text-agent-working" aria-hidden="true" />;
  return <FileText className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />;
}

/** One folder's rows: its sub-folders (each foldable, with how many documents it holds), then its documents by title. */
function FolderRows({ folder, depth, open, closed, onToggle, onDoc }: { folder: WikiFolder; depth: number; open?: string; closed: ReadonlySet<string>; onToggle: (path: string) => void; onDoc: (path: string) => void }) {
  const indent = { paddingLeft: 8 + depth * 12 };
  return (
    <>
      {folder.folders.map((sub) => {
        const expanded = !closed.has(sub.path);
        return (
          <div key={sub.path}>
            <button type="button" onClick={() => onToggle(sub.path)} aria-expanded={expanded} style={indent} className={ROW}>
              <ChevronRight className={`size-3 shrink-0 text-muted-foreground transition-transform ${expanded ? "rotate-90" : ""}`} aria-hidden="true" />
              <Folder className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
              <span className="min-w-0 flex-1 truncate">{sub.name}</span>
              <span className="shrink-0 text-2xs text-muted-foreground">{countDocs(sub)}</span>
            </button>
            {expanded && <FolderRows folder={sub} depth={depth + 1} open={open} closed={closed} onToggle={onToggle} onDoc={onDoc} />}
          </div>
        );
      })}
      {folder.docs.map((doc) => (
        <button key={doc.path} type="button" onClick={() => onDoc(doc.path)} aria-current={doc.path === open ? "page" : undefined} title={`${doc.title}\n${timeText(doc.at)}`} style={indent} className={ROW}>
          <span className="size-3 shrink-0" aria-hidden="true" />
          <DocIcon kind={doc.kind} />
          <span className="min-w-0 flex-1 truncate">{doc.title}</span>
        </button>
      ))}
    </>
  );
}

/** What the document is and where it came from; a material can be taken out here (asked twice). */
function PageHead({ page, onRemove }: { page: DreamPage; onRemove: (path: string) => Promise<void> }) {
  const [asking, setAsking] = useState(false);
  const [error, setError] = useState<string>();
  const source = page.source && safeUrl(page.source);
  const openOutside = (e: MouseEvent) => {
    if (!source || !window.__TAURI_INTERNALS__) return;
    e.preventDefault();
    void backend().openExternal(source).catch(() => {});
  };
  return (
    <header className="mb-4 border-b pb-3 text-2xs text-muted-foreground">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <span className="flex items-center gap-1 font-medium text-foreground">
          <DocIcon kind={page.kind} />
          {KIND_WORD[page.kind]}
        </span>
        <span>{timeText(page.at)}</span>
        {page.project && <span className="min-w-0 truncate font-mono" title={page.project}>{page.project}</span>}
        {page.kind === "external" && (
          <span className="ml-auto flex items-center gap-1.5">
            {asking && <span>이 자료를 뺄까요?</span>}
            <button
              type="button"
              onClick={() => {
                if (!asking) return setAsking(true);
                setError(undefined);
                onRemove(page.path).catch((e) => setError(errorText(e)));
              }}
              onBlur={() => setAsking(false)}
              className={`flex h-6 items-center gap-1 rounded px-1.5 focus-ring ${asking ? "bg-destructive/10 text-destructive" : "hover:bg-accent hover:text-foreground"}`}
            >
              <Trash2 className="size-3" aria-hidden="true" />
              {asking ? "빼기" : "자료 빼기"}
            </button>
          </span>
        )}
      </div>
      {page.source && (
        <p className="mt-1 min-w-0 truncate">
          출처:{" "}
          {source ? (
            <a href={source} target="_blank" rel="noreferrer noopener" onClick={openOutside} className="underline underline-offset-2 hover:text-foreground">
              {page.source}
            </a>
          ) : (
            page.source
          )}
        </p>
      )}
      {/* A material that opens with its own heading already shows its title. */}
      {page.kind === "external" && !/^#\s/.test(page.text) && <h1 className="mt-2 text-base font-medium text-foreground">{page.title}</h1>}
      {error && <p role="alert" className="mt-1 text-destructive">자료를 빼지 못했습니다: {error}</p>}
    </header>
  );
}

/** The document's Markdown. A link to another listed document opens it here; a web link opens outside; images are not fetched. */
const Document = memo(function Document({ page, paths, onDoc }: { page: DreamPage; paths: string[]; onDoc: (path: string) => void }) {
  const components = useMemo<Components>(
    () => ({
      a: ({ href, children }) => {
        const target = href ? resolveLink(page.path, href, paths) : undefined;
        if (target) {
          const go = (e: MouseEvent) => {
            e.preventDefault();
            onDoc(target);
          };
          return (
            <a href={`#${target}`} title={target} onClick={go}>
              {children}
            </a>
          );
        }
        const outside = href ? safeUrl(href) : undefined;
        if (!outside) return <span>{children}</span>;
        const openOutside = (e: MouseEvent) => {
          if (!window.__TAURI_INTERNALS__) return;
          e.preventDefault();
          void backend().openExternal(outside).catch(() => {});
        };
        return (
          <a href={outside} target="_blank" rel="noreferrer noopener" title={outside} onClick={openOutside}>
            {children}
          </a>
        );
      },
      img: ({ alt }) => <span className="text-muted-foreground">[이미지{alt ? `: ${alt}` : ""}]</span>,
      table: ({ children }) => (
        <div className="md-scroll" tabIndex={0}>
          <table>{children}</table>
        </div>
      ),
      pre: ({ children }) => (
        <pre className="md-scroll" tabIndex={0}>
          {children}
        </pre>
      ),
    }),
    [page.path, paths, onDoc],
  );
  return (
    <div className="chat-markdown">
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>
        {page.text}
      </ReactMarkdown>
    </div>
  );
});

function LinkList({ title, paths, docs, onDoc }: { title: string; paths: string[]; docs: DreamDoc[]; onDoc: (path: string) => void }) {
  if (paths.length === 0) return null;
  const byPath = new Map(docs.map((d) => [d.path, d]));
  return (
    <section className="mt-6 border-t pt-3">
      <h2 className="text-xs font-medium text-muted-foreground">
        {title} {paths.length}
      </h2>
      <ul className="mt-1.5 space-y-0.5">
        {paths.map((path) => {
          const doc = byPath.get(path);
          return (
            <li key={path}>
              <button type="button" onClick={() => onDoc(path)} className="flex w-full items-center gap-2 rounded px-1.5 py-1 text-left text-xs hover:bg-accent focus-ring">
                {doc && <DocIcon kind={doc.kind} />}
                <span className="min-w-0 truncate font-medium">{doc?.title ?? fileName(path)}</span>
                {doc && <span className="ml-auto shrink-0 text-2xs text-muted-foreground">{KIND_WORD[doc.kind]}</span>}
              </button>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
