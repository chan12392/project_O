import { BookOpen, BrainCircuit, Waypoints, type LucideIcon } from "lucide-react";
import { useState } from "react";

import type { Project } from "../../data/projects.ts";
import type { Backend } from "../../platform/backend/index.ts";
import { GraphScreen } from "../graph/GraphScreen.tsx";
import { WikiScreen } from "../wiki/WikiScreen.tsx";
import { DreamView } from "./DreamView.tsx";

type Tab = "graph" | "wiki" | "dreaming";

const TABS: { id: Tab; label: string; icon: LucideIcon }[] = [
  { id: "graph", label: "Graph", icon: Waypoints },
  { id: "wiki", label: "Wiki", icon: BookOpen },
  { id: "dreaming", label: "Dreaming", icon: BrainCircuit },
];

/**
 * The Dreaming screen (ADR-0041), in three tabs. Graph is the code graph (ADR-0033).
 * Wiki lists what Dreaming holds — materials people put in and a record per finished card —
 * and Dreaming draws those documents as one network; one picked there opens in Wiki.
 */
export function DreamingScreen({ api, projects, current }: { api: Backend; projects: Project[]; current?: string }) {
  const [tab, setTab] = useState<Tab>("graph");
  const [doc, setDoc] = useState<string>();

  return (
    <section className="flex min-h-0 min-w-0 flex-1 flex-col" aria-label="Dreaming">
      <div role="tablist" aria-label="Dreaming" className="flex h-9 shrink-0 items-end gap-1 border-b bg-tab-bar px-2">
        {TABS.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            type="button"
            role="tab"
            id={`dreaming-tab-${id}`}
            aria-selected={tab === id}
            aria-controls="dreaming-panel"
            onClick={() => setTab(id)}
            className="-mb-px flex h-8 items-center gap-1.5 rounded-t-md border border-transparent px-3 text-[13px] text-muted-foreground hover:text-foreground focus-ring aria-selected:border-border aria-selected:border-b-background aria-selected:bg-background aria-selected:font-medium aria-selected:text-foreground"
          >
            <Icon className="size-3.5" aria-hidden="true" />
            {label}
          </button>
        ))}
      </div>
      <div role="tabpanel" id="dreaming-panel" aria-labelledby={`dreaming-tab-${tab}`} className="flex min-h-0 flex-1 flex-col">
        {tab === "graph" && <GraphScreen api={api} projects={projects} current={current} />}
        {tab === "wiki" && <WikiScreen api={api} projects={projects} doc={doc} onDoc={setDoc} />}
        {tab === "dreaming" && (
          <DreamView
            api={api}
            onOpenDoc={(path) => {
              setDoc(path);
              setTab("wiki");
            }}
          />
        )}
      </div>
    </section>
  );
}
