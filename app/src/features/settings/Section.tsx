/** The Settings section contract and the layout pieces every section is drawn with. */
import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";

import type { Backend } from "../../platform/backend/index.ts";

export type About = Awaited<ReturnType<Backend["about"]>>;

/** What `useAbout` read: the version and folders, or why they could not be read. */
export interface AboutState {
  data?: About;
  error?: string;
}

/** What the Settings screen hands every section. */
export interface SectionContext {
  api: Backend;
  /** Where the screen's runs come from, for 정보. */
  source: "live" | "demo" | "pending";
  about: AboutState;
  onOnboarding: () => void;
}

/** One Settings section: its nav entry and what it draws (sections/index.ts lists them). */
export interface SettingsSectionDef {
  id: string;
  title: string;
  icon: LucideIcon;
  render: (ctx: SectionContext) => ReactNode;
}

export function Section({ title, description, children }: { title: string; description: string; children: ReactNode }) {
  return (
    <section aria-labelledby="settings-section-title" className="space-y-6">
      <header className="space-y-2 border-b pb-5">
        <h1 id="settings-section-title" className="text-2xl font-semibold tracking-[-0.01em]">
          {title}
        </h1>
        <p className="text-sm text-muted-foreground">{description}</p>
      </header>
      {children}
    </section>
  );
}

/** Rows separated by hairlines, the way Orca lists a section's settings. */
export function Rows({ children }: { children: ReactNode }) {
  return <div className="divide-y">{children}</div>;
}

export function Path({ path }: { path: string }) {
  return <span className="font-mono text-2xs break-all text-foreground/85">{path}</span>;
}
