import { MessagesSquare } from "lucide-react";
import { useState } from "react";

import { EFFORT_WORD, LEAD_EFFORTS, LEAD_NAME, LEAD_PROVIDERS, MODEL_ID, effortFor, type LeadProvider } from "../../../../../src/core/lead.ts";
import { updateSettings, useSettings } from "../../../data/settings.ts";
import { useLeadModels } from "../../../data/useLead.ts";
import type { Backend } from "../../../platform/backend/index.ts";
import { Segmented, SettingsRow } from "../../../ui/controls.tsx";
import { Rows, Section, type SettingsSectionDef } from "../Section.tsx";

export const LEAD_SECTION = {
  id: "lead",
  title: "총괄",
  icon: MessagesSquare,
  render: ({ api }) => <LeadSection api={api} />,
} as const satisfies SettingsSectionDef;

const FIELD = "h-8 w-56 rounded-md border bg-background/50 px-2 text-xs focus-ring disabled:cursor-not-allowed disabled:opacity-50";

function LeadSection({ api }: { api: Backend }) {
  const settings = useSettings();
  const agent = settings.leadAgent;
  return (
    <Section title="총괄" description="총괄 대화에서 새 메시지가 처음 쓰는 에이전트·모델·노력 수준입니다. 보낼 때마다 입력창에서 바꿀 수 있습니다.">
      <Rows>
        <SettingsRow
          label="기본 총괄 에이전트"
          description="총괄은 프로젝트를 읽기만 하고 작업카드 계획을 제안합니다. 읽기 전용 실행과 이어 말하기를 보장하는 Claude Code·Codex·Antigravity가 총괄을 맡습니다. 연결되지 않은 에이전트는 입력창에서 고를 수 없고 이유가 함께 보입니다."
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
        {LEAD_PROVIDERS.map((id) => (
          <SettingsRow
            key={id}
            id={`lead-model-${id}`}
            label={`${LEAD_NAME[id]} 기본 모델`}
            description={MODEL_SOURCE[id]}
            control={<DefaultModel api={api} agent={id} />}
          />
        ))}
      </Rows>
    </Section>
  );
}

/** Where each agent's model list comes from, in the row's description. */
const MODEL_SOURCE: Record<LeadProvider, string> = {
  claude: "Claude Code가 문서화한 모델 별칭(최신 Opus·Sonnet 등)입니다. 기본 모델은 Claude Code 설정을 따릅니다.",
  codex: "codex debug models가 알려 주는 모델 목록입니다. 기본 모델은 Codex 설정을 따릅니다.",
  agy: "agy models가 알려 주는 모델 목록입니다. 기본 모델은 Antigravity 설정을 따릅니다.",
};

/** One agent's default model: a list from the CLI, or a typed id when the CLI could not list them. */
function DefaultModel({ api, agent }: { api: Backend; agent: LeadProvider }) {
  const settings = useSettings();
  const saved = settings.leadModels[agent] ?? "";
  const { loading, models, failed } = useLeadModels(api, agent);
  const [error, setError] = useState<string>();
  const save = (id: string) => {
    const next = { ...settings.leadModels };
    if (id) next[agent] = id;
    else delete next[agent];
    updateSettings({ leadModels: next });
  };
  const label = `${LEAD_NAME[agent]} 기본 모델`;

  if (failed) {
    const commit = (value: string) => {
      const id = value.trim();
      if (id && !MODEL_ID.test(id)) return setError("영문·숫자로 시작하는 80자 이내의 모델 id만 받습니다");
      setError(undefined);
      save(id);
    };
    return (
      <div className="flex w-56 flex-col items-end gap-1">
        <input
          key={saved}
          aria-label={`${label} (직접 입력, 비우면 기본 모델)`}
          defaultValue={saved}
          placeholder="기본 모델"
          spellCheck={false}
          autoComplete="off"
          aria-invalid={error ? true : undefined}
          aria-describedby={`lead-model-${agent}-note`}
          onBlur={(e) => commit(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && commit(e.currentTarget.value)}
          className={`${FIELD} font-mono aria-invalid:border-destructive`}
        />
        <p id={`lead-model-${agent}-note`} className={`text-2xs ${error ? "text-destructive" : "text-muted-foreground"}`}>
          {error ?? "목록을 불러오지 못해 id를 직접 받습니다"}
        </p>
      </div>
    );
  }
  return (
    <select aria-label={label} value={saved} disabled={loading} onChange={(e) => save(e.target.value)} className={FIELD}>
      <option value="">{loading ? "목록을 불러오는 중…" : "기본 모델"}</option>
      {models.map((m) => (
        <option key={m.id} value={m.id}>
          {m.label}
        </option>
      ))}
      {saved && !models.some((m) => m.id === saved) && <option value={saved}>{saved}</option>}
    </select>
  );
}
