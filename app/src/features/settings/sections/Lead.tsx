import { MessagesSquare } from "lucide-react";

import { EFFORT_WORD, LEAD_EFFORTS, LEAD_PROVIDERS, effortFor, type LeadProvider } from "../../../../../src/core/lead.ts";
import { updateSettings, useSettings } from "../../../data/settings.ts";
import { Segmented, SettingsRow } from "../../../ui/controls.tsx";
import { Rows, Section, type SettingsSectionDef } from "../Section.tsx";

const LEAD_NAME: Record<LeadProvider, string> = { claude: "Claude Code", codex: "Codex" };

export const LEAD_SECTION = {
  id: "lead",
  title: "총괄",
  icon: MessagesSquare,
  render: () => <LeadSection />,
} as const satisfies SettingsSectionDef;

function LeadSection() {
  const settings = useSettings();
  const agent = settings.leadAgent;
  return (
    <Section title="총괄" description="총괄 대화에서 새 메시지가 처음 쓰는 에이전트와 노력 수준입니다. 보낼 때마다 입력창에서 바꿀 수 있습니다.">
      <Rows>
        <SettingsRow
          label="기본 총괄 에이전트"
          description="총괄은 프로젝트를 읽기만 하고 작업카드 계획을 제안합니다. 읽기 전용 실행과 이어 말하기를 보장하는 Claude Code와 Codex만 총괄을 맡습니다. 연결되지 않은 에이전트는 입력창에 보이지 않습니다."
          control={
            <Segmented<LeadProvider>
              label="기본 총괄 에이전트"
              value={agent}
              onChange={(leadAgent) => updateSettings({ leadAgent, leadEffort: effortFor(leadAgent, settings.leadEffort) })}
              options={LEAD_PROVIDERS.map((id) => ({ value: id, label: LEAD_NAME[id] }))}
            />
          }
        />
        <SettingsRow
          label="기본 노력 수준"
          description="높을수록 계획이 꼼꼼해지지만 총괄 한 번에 드는 토큰도 늘어납니다. 카드마다의 에이전트·노력 수준은 총괄이 일에 맞게 고릅니다."
          control={
            <Segmented
              label="기본 노력 수준"
              value={settings.leadEffort}
              onChange={(leadEffort) => updateSettings({ leadEffort })}
              options={LEAD_EFFORTS[agent].map((e) => ({ value: e, label: EFFORT_WORD[e] ?? e }))}
            />
          }
        />
      </Rows>
    </Section>
  );
}
