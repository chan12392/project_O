import { LoaderCircle, Pencil, Plus, X } from "lucide-react";
import { useRef, useState, type KeyboardEvent } from "react";

import { LEAD_TAB_MAX, LEAD_TAB_WORD, type LeadTab } from "../../../../src/core/lead.ts";
import type { LeadState } from "../../data/useLead.ts";
import { useDismiss } from "../../lib/dismiss.ts";
import { LeadStateDot } from "../../ui/LeadStateDot.tsx";

interface LeadTabsProps {
  /** "project_O 총괄", "전체 총괄" — the strip's leading name. */
  name: string;
  /** The name's tooltip: the folder the lead works on. */
  path?: string;
  lead: LeadState;
  /** "+": opens a new tab and selects it; rejects with the reason. */
  onNew: () => Promise<void>;
  onSelect: (threadId: string) => void;
  onClose: (tab: LeadTab) => Promise<void>;
  onWriter: (tab: LeadTab) => Promise<void>;
  /** Where a failed action's reason lands (the header's alert line). */
  onError: (message: string) => void;
  /** "+" needs a project on screen. */
  canNew: boolean;
}

/**
 * The header's open-conversation tabs (ADR-0034): the project's name, one tab per open lead
 * thread — state dot, its title, the writer's pencil, a close × that appears on hover or
 * focus and is disabled while that thread answers — and "+" for a new one, capped at four.
 * Right-click on a non-writer tab offers "직접 수정 맡기기". Tabs are a `tablist`: arrow keys
 * (and Home/End) move between them and follow the pick; the strip scrolls inside the header.
 */
export function LeadTabs({ name, path, lead, onNew, onSelect, onClose, onWriter, onError, canNew }: LeadTabsProps) {
  const [adding, setAdding] = useState(false);
  const [closing, setClosing] = useState<ReadonlySet<string>>(new Set());
  const [menuFor, setMenuFor] = useState<string>();
  const listRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  useDismiss(menuRef, menuFor !== undefined, () => setMenuFor(undefined));
  const tabs = lead.tabs;
  const reason = (e: unknown) => (e instanceof Error ? e.message : String(e));
  const maxed = tabs.length >= LEAD_TAB_MAX;

  const add = async () => {
    setAdding(true);
    try {
      await onNew();
    } catch (e) {
      onError(`새 총괄을 열지 못했습니다: ${reason(e)}`);
    } finally {
      setAdding(false);
    }
  };

  const close = async (tab: LeadTab) => {
    setClosing((s) => new Set(s).add(tab.threadId));
    try {
      await onClose(tab);
    } catch (e) {
      onError(`총괄 탭을 닫지 못했습니다: ${reason(e)}`);
    } finally {
      setClosing((s) => {
        const next = new Set(s);
        next.delete(tab.threadId);
        return next;
      });
    }
  };

  // Arrow keys (and Home/End) move between tabs and follow the pick: roving tabindex, automatic activation.
  const move = (e: KeyboardEvent<HTMLDivElement>) => {
    const at = tabs.findIndex((t) => t.threadId === (e.target as HTMLElement).dataset.thread);
    if (at < 0) return;
    const next =
      e.key === "ArrowRight" ? (at + 1) % tabs.length : e.key === "ArrowLeft" ? (at - 1 + tabs.length) % tabs.length : e.key === "Home" ? 0 : e.key === "End" ? tabs.length - 1 : -1;
    if (next < 0 || next === at) return;
    e.preventDefault();
    const id = tabs[next].threadId;
    onSelect(id);
    listRef.current?.querySelector<HTMLElement>(`[data-thread="${id.replaceAll('"', "")}"]`)?.focus();
  };

  return (
    <div className="flex min-w-0 flex-1 items-center gap-1.5">
      <span className="min-w-0 truncate font-medium text-tab-active-foreground" title={path}>
        {name}
      </span>
      <div role="tablist" aria-label="총괄 대화" ref={listRef} onKeyDown={move} className="scrollbar-none flex min-w-0 items-center gap-0.5 overflow-x-auto">
        {tabs.map((tab) => {
          const title = tab.title || "새 총괄";
          const selected = lead.selected === tab.threadId;
          const blocked = tab.busy === "unknown" ? "이전 답변이 끝났는지 확인할 수 없어 지금은 닫을 수 없습니다" : "답변하는 동안에는 닫을 수 없습니다";
          const tip = [title, LEAD_TAB_WORD[tab.state], tab.writer && "이 총괄만 프로젝트 파일을 직접 고칩니다", tab.busy && blocked].filter(Boolean).join(" — ");
          return (
            <span
              key={tab.threadId}
              className="group/tab relative flex max-w-44 shrink-0 items-center"
              onContextMenu={tab.writer ? undefined : (e) => {
                e.preventDefault();
                setMenuFor(tab.threadId);
              }}
            >
              <button
                type="button"
                role="tab"
                data-thread={tab.threadId}
                aria-selected={selected}
                aria-controls="lead-chat-panel"
                aria-label={`${title} 탭, ${LEAD_TAB_WORD[tab.state]}${tab.writer ? ", 직접 수정 총괄" : ""}`}
                title={tip}
                tabIndex={selected ? 0 : -1}
                onClick={() => onSelect(tab.threadId)}
                className="flex h-7 min-w-0 items-center gap-1.5 rounded-md pr-5 pl-2 text-xs whitespace-nowrap text-tab-inactive-foreground hover:bg-accent/60 hover:text-tab-active-foreground focus-ring aria-selected:bg-tab-active aria-selected:font-medium aria-selected:text-tab-active-foreground"
              >
                <LeadStateDot state={tab.state} />
                <span className="truncate">{title}</span>
                {tab.writer && (
                  <span title="이 총괄만 프로젝트 파일을 직접 고칩니다">
                    <Pencil className="size-3 shrink-0" aria-hidden="true" />
                  </span>
                )}
              </button>
              <button
                type="button"
                aria-label={`${title} 탭 닫기`}
                title={tab.busy ? blocked : `${title} 탭을 닫습니다 (기록은 남습니다)`}
                disabled={Boolean(tab.busy) || closing.has(tab.threadId)}
                onClick={() => void close(tab)}
                className="absolute right-0.5 flex size-4 items-center justify-center rounded-sm opacity-0 hover:bg-accent focus-ring group-hover/tab:opacity-100 group-focus-within/tab:opacity-100 focus-visible:opacity-100 disabled:cursor-not-allowed disabled:opacity-30 disabled:hover:bg-transparent"
              >
                {closing.has(tab.threadId) ? <LoaderCircle className="size-3 animate-spin" aria-hidden="true" /> : <X className="size-3" aria-hidden="true" />}
              </button>
              {menuFor === tab.threadId && (
                <div ref={menuRef} role="menu" className="absolute top-full left-0 z-30 mt-0.5 w-40 rounded-md border bg-popover p-1 text-popover-foreground shadow-lg">
                  <button
                    type="button"
                    role="menuitem"
                    className="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-xs hover:bg-accent focus-ring"
                    onClick={() => {
                      setMenuFor(undefined);
                      void onWriter(tab).catch((e: unknown) => onError(`직접 수정을 맡기지 못했습니다: ${reason(e)}`));
                    }}
                  >
                    <Pencil className="size-3.5" aria-hidden="true" />
                    직접 수정 맡기기
                  </button>
                </div>
              )}
            </span>
          );
        })}
      </div>
      <button
        type="button"
        aria-label="새 총괄 대화"
        title={maxed ? "열린 총괄은 최대 4개입니다" : "새 총괄 대화를 엽니다"}
        disabled={!canNew || maxed || adding}
        onClick={() => void add()}
        className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md hover:bg-accent focus-ring disabled:cursor-not-allowed disabled:opacity-40"
      >
        {adding ? <LoaderCircle className="size-3.5 animate-spin" aria-hidden="true" /> : <Plus className="size-3.5" aria-hidden="true" />}
      </button>
    </div>
  );
}
