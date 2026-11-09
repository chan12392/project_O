import { BookOpen } from "lucide-react";

import { Section, type SettingsSectionDef } from "../Section.tsx";

export const WIKI_SECTION = {
  id: "wiki",
  title: "Wiki",
  icon: BookOpen,
  render: () => <WikiSection />,
} as const satisfies SettingsSectionDef;

function WikiSection() {
  return (
    <Section title="Wiki" description="프로젝트의 문서를 모아 보는 곳입니다.">
      <p className="text-sm text-muted-foreground">위키는 준비 중입니다.</p>
    </Section>
  );
}
