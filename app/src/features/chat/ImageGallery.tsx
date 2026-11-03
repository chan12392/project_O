import { MoreHorizontal, X } from "lucide-react";
import { createContext, useContext, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { backend } from "../../platform/backend/index.ts";
import { imagePaths } from "./imagePaths.ts";

export const ImageScope = createContext<{ project: string; run?: string; card?: string } | undefined>(undefined);

export function ImageGallery({ text }: { text: string }) {
  const scope = useContext(ImageScope);
  const paths = useMemo(() => imagePaths(text), [text]);
  if (!scope || !paths.length) return null;
  return <div className="mt-3 grid grid-cols-2 gap-2">{paths.map((path) => <Preview key={path} path={path} scope={scope} />)}</div>;
}

function Preview({ path, scope }: { path: string; scope: { project: string; run?: string; card?: string } }) {
  const [url, setUrl] = useState<string>();
  const [expanded, setExpanded] = useState(false);
  const [menu, setMenu] = useState(false);
  const [error, setError] = useState("");
  const panel = useRef<HTMLDivElement>(null);
  const modalMenu = useRef<HTMLDivElement>(null);
  const thumbnail = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    let live = true; let objectUrl: string | undefined;
    setUrl(undefined);
    void backend().imageRead(scope.project, path, scope.run, scope.card).then(({ mime, bytes }) => {
      if (!live) return;
      objectUrl = URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: mime }));
      setUrl(objectUrl);
    }, () => {});
    return () => { live = false; if (objectUrl) URL.revokeObjectURL(objectUrl); };
  }, [path, scope.project, scope.run, scope.card]);
  useEffect(() => {
    if (!expanded && !menu) return;
    const key = (event: KeyboardEvent) => { if (event.key === "Escape") { setExpanded(false); setMenu(false); if (expanded) requestAnimationFrame(() => thumbnail.current?.focus()); } };
    document.addEventListener("keydown", key);
    return () => document.removeEventListener("keydown", key);
  }, [expanded, menu]);
  useEffect(() => {
    if (!menu) return;
    const dismiss = (event: PointerEvent) => { if (!panel.current?.contains(event.target as Node) && !modalMenu.current?.contains(event.target as Node)) setMenu(false); };
    document.addEventListener("pointerdown", dismiss);
    return () => document.removeEventListener("pointerdown", dismiss);
  }, [menu]);
  if (!url) return null;
  const open = (folder: boolean) => { setMenu(false); void backend().imageOpen(scope.project, path, folder, scope.run, scope.card).catch((e: unknown) => setError(String(e))); };
  const closeExpanded = () => { setExpanded(false); setMenu(false); requestAnimationFrame(() => thumbnail.current?.focus()); };
  const actions = <div role="menu" className="absolute right-1 top-8 z-30 w-44 rounded-md border bg-popover p-1 text-xs text-popover-foreground shadow-lg"><button type="button" role="menuitem" className="block w-full rounded px-2 py-2 text-left hover:bg-accent focus-ring" onClick={() => open(true)}>폴더로 열기</button><button type="button" role="menuitem" className="block w-full rounded px-2 py-2 text-left hover:bg-accent focus-ring" onClick={() => open(false)}>기본 시스템으로 열기</button></div>;
  return <div ref={panel} className="relative min-w-0" onContextMenu={(e) => { e.preventDefault(); setMenu(true); }}>
    <button ref={thumbnail} type="button" onClick={() => setExpanded(true)} className="block w-full overflow-hidden rounded-md border bg-muted focus-ring" aria-label={`이미지 확대: ${path}`}><img src={url} alt={path.split(/[\\/]/).pop()} className="aspect-square w-full object-contain" /></button>
    <button type="button" onClick={() => setMenu(!menu)} aria-label="이미지 메뉴" aria-expanded={menu} className="absolute right-1 top-1 rounded bg-background/90 p-1 focus-ring"><MoreHorizontal className="size-4" /></button>
    {menu && !expanded && actions}
    {error && <p role="alert" className="text-xs text-destructive">이미지를 열지 못했습니다: {error}</p>}
    {expanded && createPortal(<div className="fixed inset-0 z-[100] flex items-center justify-center bg-black/80 p-8" onClick={closeExpanded} onKeyDown={(e) => { if (e.key !== "Tab") return; const buttons = [...e.currentTarget.querySelectorAll<HTMLButtonElement>("button")]; const at = buttons.indexOf(document.activeElement as HTMLButtonElement); if ((!e.shiftKey && at === buttons.length - 1) || (e.shiftKey && at === 0)) { e.preventDefault(); buttons[e.shiftKey ? buttons.length - 1 : 0]?.focus(); } }} role="dialog" aria-modal="true" aria-label="이미지 확대"><button autoFocus type="button" onClick={closeExpanded} aria-label="닫기" className="absolute right-5 top-5 rounded bg-background p-2 text-foreground focus-ring"><X className="size-5" /></button><div ref={modalMenu} className="absolute right-16 top-5" onClick={(e) => e.stopPropagation()}><button type="button" onClick={() => setMenu(!menu)} aria-label="이미지 메뉴" aria-expanded={menu} className="rounded bg-background p-2 text-foreground focus-ring"><MoreHorizontal className="size-5" /></button>{menu && actions}</div><img src={url} alt={path.split(/[\\/]/).pop()} onClick={(e) => e.stopPropagation()} onContextMenu={(e) => { e.preventDefault(); e.stopPropagation(); setMenu(true); }} className="max-h-full max-w-full object-contain" /></div>, document.body)}
  </div>;
}
