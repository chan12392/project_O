import { MapPin, Trash2 } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";

import type { DesignNote } from "../../../../src/core/lead.ts";
import type { LeadState } from "../../data/useLead.ts";

type Draft = { note: DesignNote; original?: DesignNote; x: number; y: number };
type Pin = { note: DesignNote; top: number };

export function DesignAnnotations({ lead, children }: { lead: LeadState; children: ReactNode }) {
  const thread = lead.thread?.threadId;
  const notes = lead.thread?.notes ?? [];
  const area = useRef<HTMLDivElement>(null);
  const popup = useRef<HTMLDivElement>(null);
  const saving = useRef(false);
  const [draft, setDraft] = useState<Draft>();
  const [pins, setPins] = useState<Pin[]>([]);
  const [error, setError] = useState("");

  useEffect(() => {
    const measure = () => {
      const root = area.current;
      if (!root) return;
      const bounds = root.getBoundingClientRect();
      const candidates = Array.from(root.querySelectorAll<HTMLElement>("p, li, td, th"));
      const next = notes.flatMap((note) => {
        const matching = candidates.filter((element) => element.offsetParent !== null && (note.anchor ? element.textContent?.trim().slice(0, 2000) === note.anchor : element.textContent?.includes(note.quote)));
        const found = matching[note.occurrence ?? 0];
        return found ? [{ note, top: Math.max(0, found.getBoundingClientRect().top - bounds.top) }] : [];
      });
      setPins((old) => JSON.stringify(old) === JSON.stringify(next) ? old : next);
    };
    measure();
    const observer = new ResizeObserver(measure);
    if (area.current) observer.observe(area.current);
    window.addEventListener("resize", measure);
    window.addEventListener("scroll", measure, true);
    return () => { observer.disconnect(); window.removeEventListener("resize", measure); window.removeEventListener("scroll", measure, true); };
  }, [notes, children]);

  const open = (note: DesignNote, x: number, y: number, original?: DesignNote) => {
    if (saving.current) return;
    setError("");
    setDraft({ note, original, x: Math.max(8, Math.min(x, window.innerWidth - 292)), y: Math.max(8, Math.min(y, window.innerHeight - 150)) });
  };
  const selected = () => {
    const selection = window.getSelection();
    return selection && area.current?.contains(selection.anchorNode) && area.current?.contains(selection.focusNode) ? selection.toString().trim().slice(0, 500) : "";
  };
  const makeNote = (quote: string, element?: Element | null): DesignNote => {
    const anchor = element?.closest("td, th, p, li")?.textContent?.trim().slice(0, 2000) ?? "";
    const matches = Array.from(area.current?.querySelectorAll<HTMLElement>("p, li, td, th") ?? []).filter((candidate) => candidate.offsetParent !== null && candidate.textContent?.trim().slice(0, 2000) === anchor);
    const occurrence = Math.max(0, matches.indexOf(element?.closest("td, th, p, li") as HTMLElement));
    return { id: crypto.randomUUID(), quote, anchor, occurrence, tag: "주석", text: "", sent: false };
  };
  const commit = async () => {
    if (!draft || !thread || saving.current) return;
    const text = draft.note.text.trim();
    if (!text) { if (draft.original) setError("내용을 입력하거나 삭제 버튼을 누르세요."); else setDraft(undefined); return; }
    if (draft.original && draft.original.text === text && draft.original.tag === draft.note.tag) { setDraft(undefined); return; }
    saving.current = true;
    try {
      await lead.changeNote(thread, { op: draft.original ? "update" : "add", note: { ...draft.note, text, sent: false } });
      setDraft(undefined);
      setError("");
    } catch (e) { setError(`저장하지 못했습니다: ${e instanceof Error ? e.message : String(e)}`); }
    finally { saving.current = false; }
  };
  useEffect(() => {
    if (!draft) return;
    const outside = (event: PointerEvent) => { if (!popup.current?.contains(event.target as Node)) void commit(); };
    document.addEventListener("pointerdown", outside, true);
    return () => document.removeEventListener("pointerdown", outside, true);
  });

  const unmatched = notes.filter((note) => !pins.some((pin) => pin.note.id === note.id));
  return <div className="space-y-3">
    {thread && <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
      <button type="button" className="rounded border px-2 py-1 hover:bg-accent focus-ring" onClick={(e) => { const quote = selected(); if (quote) { const rect = e.currentTarget.getBoundingClientRect(); open(makeNote(quote, window.getSelection()?.anchorNode?.parentElement), rect.left, rect.bottom); } else setError("설계에서 내용을 먼저 선택하세요."); }}>선택 부분에 주석</button>
      <span>내용을 우클릭하면 태그나 주석을 남길 수 있습니다.</span>
    </div>}
    {(error || lead.thread?.notesError) && <p role="alert" className="text-xs text-destructive">{error || `주석을 읽지 못했습니다: ${lead.thread?.notesError}`}</p>}
    <div className="relative">
      <div ref={area} className="space-y-4" onContextMenu={(e) => {
        if (!thread || (e.target as HTMLElement).closest("button, input, textarea, select, a")) return;
        const quote = (window.getSelection()?.containsNode(e.target as Node, true) ? selected() : "") || (e.target as HTMLElement).closest("td, th, p, li")?.textContent?.trim().slice(0, 500);
        if (quote) { e.preventDefault(); open(makeNote(quote, e.target as Element), e.clientX, e.clientY); }
      }}>{children}</div>
      {pins.map(({ note, top }, i) => <button key={note.id} type="button" aria-label={`${note.tag}: ${note.text}`} title={`${note.tag}: ${note.text}`} className={`absolute rounded bg-background p-0.5 focus-ring ${note.tag === "태그" ? "text-orange-500" : "text-red-500"}`} style={{ top, right: (i % 4) * 20 }} onClick={(e) => { const rect = e.currentTarget.getBoundingClientRect(); open(note, rect.left, rect.bottom, note); }}><MapPin className="size-4 fill-current" aria-hidden="true" /></button>)}
    </div>
    {unmatched.length > 0 && <div className="flex flex-wrap gap-2 text-xs text-muted-foreground"><span>바뀐 원문에 남긴 핀</span>{unmatched.map((note) => <button key={note.id} type="button" className="flex items-center gap-1 rounded border px-1 focus-ring" onClick={(e) => { const rect = e.currentTarget.getBoundingClientRect(); open(note, rect.left, rect.bottom, note); }}><MapPin className="size-3 text-orange-500" aria-hidden="true" />{note.text}</button>)}</div>}
    {draft && createPortal(<div ref={popup} role="dialog" aria-label="설계 태그와 주석" className="fixed z-50 w-72 rounded-lg border bg-popover p-2 text-popover-foreground shadow-lg" style={{ left: draft.x, top: draft.y }} onKeyDown={(e) => { if (e.key === "Escape") { e.stopPropagation(); setDraft(undefined); } }}>
      <div className="mb-1 flex items-center gap-1 text-xs" role="group" aria-label="종류">
        {(["태그", "주석"] as const).map((tag) => <button key={tag} type="button" aria-pressed={draft.note.tag === tag} className="rounded px-2 py-1 hover:bg-accent aria-pressed:bg-accent focus-ring" onClick={() => setDraft({ ...draft, note: { ...draft.note, tag } })}>{tag}</button>)}
        {draft.original && <button type="button" aria-label="핀 삭제" className="ml-auto rounded p-1 hover:bg-accent focus-ring" onClick={() => { if (!thread || saving.current) return; saving.current = true; void lead.changeNote(thread, { op: "delete", id: draft.note.id }).then(() => { setDraft(undefined); setError(""); }, (e) => setError(`삭제하지 못했습니다: ${String(e)}`)).finally(() => { saving.current = false; }); }}><Trash2 className="size-3.5" aria-hidden="true" /></button>}
      </div>
      <form onSubmit={(e) => { e.preventDefault(); void commit(); }}><input autoFocus aria-label={draft.note.tag} maxLength={2000} value={draft.note.text} placeholder={draft.note.tag === "태그" ? "태그 입력" : "주석 입력"} className="w-full rounded border bg-background px-2 py-1.5 text-sm focus-ring" onChange={(e) => setDraft({ ...draft, note: { ...draft.note, text: e.target.value } })} /></form>
      <p className="mt-1 line-clamp-1 text-2xs text-muted-foreground" title={draft.note.quote}>“{draft.note.quote}” · Enter 또는 바깥 클릭으로 저장</p>
    </div>, document.body)}
  </div>;
}
