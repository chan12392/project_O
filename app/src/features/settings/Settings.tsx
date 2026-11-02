import { ArrowLeft } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import type { Backend } from "../../platform/backend/index.ts";
import type { AboutState, SectionContext, SettingsSectionDef } from "./Section.tsx";
import { SETTINGS_GROUPS, type SettingsSection } from "./sections/index.ts";

const SECTIONS = SETTINGS_GROUPS.flatMap((group): readonly SettingsSectionDef[] => group.items);

interface Props {
  api: Backend;
  section: SettingsSection;
  source: SectionContext["source"];
  onSection: (section: SettingsSection) => void;
  onBack: () => void;
  onOnboarding: () => void;
}

/** Settings in the window's main area, Orca's layout: section nav on the left, one section at a time. */
export function Settings({ api, section, source, onSection, onBack, onOnboarding }: Props) {
  const back = useRef<HTMLButtonElement>(null);
  useEffect(() => back.current?.focus(), []);
  const about = useAbout(api);

  return (
    <div className="col-span-2 grid min-h-0 grid-cols-[16rem_minmax(0,1fr)] max-md:col-span-1 max-md:grid-cols-[minmax(0,1fr)] max-md:grid-rows-[auto_minmax(0,1fr)]">
      <nav className="flex min-h-0 flex-col border-r bg-sidebar text-sidebar-foreground max-md:border-r-0 max-md:border-b" aria-label="설정 항목">
        <div className="border-b px-2 py-2">
          <button ref={back} type="button" onClick={onBack} className="flex h-8 w-full items-center gap-2 rounded-md px-2 text-[13px] text-muted-foreground hover:bg-sidebar-accent hover:text-foreground focus-ring">
            <ArrowLeft className="size-4" aria-hidden="true" />
            앱으로 돌아가기
          </button>
        </div>
        <div className="scrollbar-sleek min-h-0 flex-1 overflow-y-auto px-2 py-4 max-md:flex max-md:gap-4 max-md:overflow-x-auto max-md:py-2">
          {SETTINGS_GROUPS.map((group) => (
            <div key={group.title} className="mb-5 max-md:mb-0">
              <p className="px-2 pb-1.5 text-2xs font-semibold text-muted-foreground max-md:hidden">{group.title}</p>
              <ul className="space-y-0.5 max-md:flex max-md:space-y-0 max-md:gap-0.5">
                {group.items.map((item) => {
                  const current = item.id === section;
                  return (
                    <li key={item.id}>
                      <button
                        type="button"
                        onClick={() => onSection(item.id)}
                        aria-current={current ? "page" : undefined}
                        className={`flex h-8 w-full items-center gap-2 rounded-md px-2 text-[13px] whitespace-nowrap transition-colors duration-150 focus-ring ${
                          current ? "bg-sidebar-accent font-medium text-sidebar-accent-foreground" : "text-sidebar-foreground/70 hover:bg-sidebar-accent/60 hover:text-sidebar-foreground"
                        }`}
                      >
                        <item.icon className="size-4 shrink-0" aria-hidden="true" />
                        {item.title}
                      </button>
                    </li>
                  );
                })}
              </ul>
            </div>
          ))}
        </div>
      </nav>
      <div className="scrollbar-sleek min-h-0 min-w-0 overflow-y-auto bg-background [scrollbar-gutter:stable]">
        <div key={section} className="mx-auto w-full max-w-3xl px-8 pt-10 pb-24">
          {SECTIONS.find((s) => s.id === section)?.render({ api, source, about, onOnboarding })}
        </div>
      </div>
    </div>
  );
}

function useAbout(api: Backend) {
  const [state, setState] = useState<AboutState>({});
  useEffect(() => {
    let live = true;
    api.about().then(
      (data) => live && setState({ data }),
      (e: unknown) => live && setState({ error: e instanceof Error ? e.message : String(e) }),
    );
    return () => {
      live = false;
    };
  }, [api]);
  return state;
}
