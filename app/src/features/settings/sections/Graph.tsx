import { Waypoints } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { MODEL_ID } from "../../../../../src/core/lead.ts";
import { useLeadModels } from "../../../data/useLead.ts";
import { errorText } from "../../../lib/format.ts";
import type { Backend, GraphColorBy, GraphSettings } from "../../../platform/backend/index.ts";
import { BUTTON_SECONDARY, Segmented, SettingsRow, Switch } from "../../../ui/controls.tsx";
import { Rows, Section, type SettingsSectionDef } from "../Section.tsx";

export const GRAPH_SECTION = {
  id: "graph",
  title: "Graph",
  icon: Waypoints,
  render: ({ api }) => <GraphSection api={api} />,
} as const satisfies SettingsSectionDef;

const FIELD = "h-8 w-56 rounded-md border bg-background/50 px-2 text-xs focus-ring disabled:cursor-not-allowed disabled:opacity-50";

/** The official CLIs Deep may call (`GraphSettings.deepProvider`). */
type DeepProvider = GraphSettings["deepProvider"];

const PROVIDER_NAME: Record<DeepProvider, string> = { codex: "Codex", claude: "Claude" };

/** Settings → Graph: what the 3D graph draws and how Deep runs (`o graph settings`, ADR-0033). */
function GraphSection({ api }: { api: Backend }) {
  const [settings, setSettings] = useState<GraphSettings>();
  const [error, setError] = useState<string>();
  const [attempt, setAttempt] = useState(0);
  const [saving, setSaving] = useState(false);
  const [note, setNote] = useState<{ text: string; bad: boolean }>();
  const noteTimer = useRef(0);

  useEffect(() => {
    let live = true;
    setError(undefined);
    api.graphSettings().then(
      (next) => {
        if (live) setSettings(next);
      },
      (e) => {
        if (live) setError(errorText(e));
      },
    );
    return () => {
      live = false;
    };
  }, [api, attempt]);

  useEffect(() => () => window.clearTimeout(noteTimer.current), []);

  const showNote = (text: string, bad: boolean) => {
    window.clearTimeout(noteTimer.current);
    setNote({ text, bad });
    if (!bad) noteTimer.current = window.setTimeout(() => setNote(undefined), 2_000);
  };

  const save = async (change: Partial<GraphSettings>) => {
    setSaving(true);
    try {
      setSettings(await api.graphSettings(change));
      showNote("저장했습니다", false);
    } catch (e) {
      showNote(`저장하지 못했습니다: ${errorText(e)}`, true);
    } finally {
      setSaving(false);
    }
  };

  return (
    <Section title="Graph" description="프로젝트의 코드 그래프를 3D로 봅니다. 빛나는 곳은 지금 바뀐 파일과 그 영향 경로입니다. Deep은 로그인된 에이전트의 구독 사용량을 씁니다.">
      {!settings && !error && (
        <div className="divide-y" role="status" aria-label="설정을 읽는 중">
          {[0, 1, 2, 3, 4, 5, 6, 7].map((i) => (
            <div key={i} className="flex items-center justify-between gap-6 py-3.5">
              <div className="h-3.5 w-44 animate-pulse rounded bg-muted" />
              <div className="h-4 w-10 animate-pulse rounded-full bg-muted" />
            </div>
          ))}
        </div>
      )}
      {error && !settings && (
        <div role="alert" className="flex flex-wrap items-center gap-3 py-3">
          <p className="text-sm text-destructive">설정을 읽지 못했습니다: {error}</p>
          <button type="button" onClick={() => setAttempt((n) => n + 1)} className={BUTTON_SECONDARY}>
            다시 시도
          </button>
        </div>
      )}
      {settings && (
        <>
          <Rows>
            <SettingsRow
              id="graph-effects-label"
              label="효과"
              description="바뀐 곳과 그 영향 경로를 밝게 표시합니다."
              control={<Switch checked={settings.effects} disabled={saving} onChange={(effects) => void save({ effects })} labelledBy="graph-effects-label" />}
            />
            <SettingsRow
              id="graph-particles-label"
              label="흐름"
              description="빛나는 경로 위를 점이 흐릅니다."
              control={<Switch checked={settings.particles} disabled={saving} onChange={(particles) => void save({ particles })} labelledBy="graph-particles-label" />}
            />
            <SettingsRow
              label="색 기준"
              description="파일과 영역 노드를 언어(확장자)별로 색칠할지, 영역별로 색칠할지 정합니다."
              control={
                <Segmented<GraphColorBy>
                  label="색 기준"
                  value={settings.colorBy ?? "language"}
                  disabled={saving}
                  onChange={(colorBy) => void save({ colorBy })}
                  options={[
                    { value: "language", label: "언어별" },
                    { value: "area", label: "영역별" },
                  ]}
                />
              }
            />
            <SettingsRow
              id="graph-warn-deep-label"
              label="Deep 사용량 안내"
              description="Deep을 처음 실행할 때 LLM 사용량 안내를 보여 줍니다."
              control={<Switch checked={settings.warnDeep} disabled={saving} onChange={(warnDeep) => void save({ warnDeep })} labelledBy="graph-warn-deep-label" />}
            />
            <SettingsRow
              label="Deep 에이전트"
              description="Deep이 영역 이름·요약을 만들 때 부르는 공식 CLI입니다."
              control={
                <Segmented<DeepProvider>
                  label="Deep 에이전트"
                  value={settings.deepProvider}
                  disabled={saving}
                  onChange={(deepProvider) => void save({ deepProvider })}
                  options={[
                    { value: "codex", label: "Codex" },
                    { value: "claude", label: "Claude" },
                  ]}
                />
              }
            />
            {(["codex", "claude"] as const).map((provider) => (
              <SettingsRow
                key={provider}
                id={`graph-model-${provider}`}
                label={`${PROVIDER_NAME[provider]} 모델`}
                description={`Deep이 ${PROVIDER_NAME[provider]}로 실행될 때 쓰는 모델입니다. 비우면 ${provider === "codex" ? "Codex CLI에 내장된 기본 모델을 씁니다(Deep은 격리를 위해 Codex 사용자 설정을 읽지 않습니다)" : "Claude Code의 기본 모델을 씁니다"}.`}
                control={<GraphModel api={api} provider={provider} saved={provider === "codex" ? settings.codexModel : settings.claudeModel} disabled={saving} onSave={(id) => void save(provider === "codex" ? { codexModel: id } : { claudeModel: id })} />}
              />
            ))}
            <SettingsRow
              label="최대 노드 수"
              description="한 화면에 그리는 노드의 상한입니다(100–5000). 넘는 노드는 잘라내지만 빛나는 노드는 항상 남깁니다."
              control={<MaxNodes value={settings.maxNodes} disabled={saving} onSave={(maxNodes) => void save({ maxNodes })} />}
            />
            <SettingsRow
              label="영향 경로 깊이"
              description="바뀐 파일에서 영향 경로를 몇 단계까지 거슬러 올라가는지 정합니다."
              control={
                <Segmented
                  label="영향 경로 깊이"
                  value={String(settings.depth)}
                  disabled={saving}
                  onChange={(d) => void save({ depth: Number(d) })}
                  options={["1", "2", "3", "4"].map((d) => ({ value: d, label: `${d}단계` }))}
                />
              }
            />
          </Rows>
          {note && (
            <p role={note.bad ? "alert" : "status"} className={`mt-2 text-xs ${note.bad ? "text-destructive" : "text-muted-foreground"}`}>
              {note.text}
            </p>
          )}
        </>
      )}
    </Section>
  );
}

/** One Deep agent's model: a list from the CLI, or a typed id when the CLI could not list them. */
function GraphModel({ api, provider, saved, disabled, onSave }: { api: Backend; provider: DeepProvider; saved: string; disabled: boolean; onSave: (id: string) => void }) {
  const { loading, models, failed } = useLeadModels(api, provider);
  const [error, setError] = useState<string>();
  const label = `${PROVIDER_NAME[provider]} 모델`;

  if (failed) {
    const commit = (value: string) => {
      const id = value.trim();
      if (id && !MODEL_ID.test(id)) return setError("영문·숫자로 시작하는 80자 이내의 모델 id만 받습니다");
      setError(undefined);
      onSave(id);
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
          disabled={disabled}
          aria-invalid={error ? true : undefined}
          aria-describedby={`graph-model-${provider}-note`}
          onBlur={(e) => commit(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && commit(e.currentTarget.value)}
          className={`${FIELD} font-mono aria-invalid:border-destructive`}
        />
        <p id={`graph-model-${provider}-note`} className={`text-2xs ${error ? "text-destructive" : "text-muted-foreground"}`}>
          {error ?? "목록을 불러오지 못해 id를 직접 받습니다"}
        </p>
      </div>
    );
  }
  return (
    <select aria-label={label} value={saved} disabled={loading || disabled} onChange={(e) => onSave(e.target.value)} className={FIELD}>
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

/** 최대 노드 수: a number that commits on blur/Enter, clamped into 100–5000; anything not an integer is ignored. */
function MaxNodes({ value, disabled, onSave }: { value: number; disabled: boolean; onSave: (maxNodes: number) => void }) {
  const commit = (input: HTMLInputElement) => {
    const n = input.valueAsNumber;
    if (!Number.isInteger(n)) {
      input.value = String(value);
      return;
    }
    const next = Math.min(5000, Math.max(100, n));
    input.value = String(next);
    if (next !== value) onSave(next);
  };
  return (
    <input
      key={value}
      aria-label="최대 노드 수"
      type="number"
      min={100}
      max={5000}
      step={1}
      defaultValue={value}
      disabled={disabled}
      onBlur={(e) => commit(e.currentTarget)}
      onKeyDown={(e) => e.key === "Enter" && commit(e.currentTarget)}
      className={`${FIELD} w-28 tabular-nums`}
    />
  );
}
