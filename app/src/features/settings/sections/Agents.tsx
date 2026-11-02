import { Bot, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";

import { updateSettings, useSettings } from "../../../data/settings.ts";
import { BUDGET_PERCENTS, type Backend, type BudgetPercent, type BudgetReport } from "../../../platform/backend/index.ts";
import { BUTTON_SECONDARY, Segmented, SettingsRow, SubHeading, Switch } from "../../../ui/controls.tsx";
import { AgentList, useAgents } from "../AgentSetup.tsx";
import { Path, Rows, Section, type SettingsSectionDef } from "../Section.tsx";

export const AGENTS_SECTION = {
  id: "agents",
  title: "에이전트",
  icon: Bot,
  render: ({ api, about, onOnboarding }) => <AgentsSection api={api} agentsHome={about.data?.agentsHome} onOnboarding={onOnboarding} />,
} as const satisfies SettingsSectionDef;

function AgentsSection({ api, agentsHome, onOnboarding }: { api: Backend; agentsHome?: string; onOnboarding: () => void }) {
  const settings = useSettings();
  const agents = useAgents(api, settings.clean);
  return (
    <Section title="에이전트" description="작업카드를 맡을 공식 CLI의 설치·연결 상태와 실행 프로필입니다.">
      <Rows>
        <SettingsRow
          id="clean-label"
          label="project_O 프로필 사용"
          description={
            <>
              켜면 작업카드와 총괄을 Project O의 깨끗한 에이전트 프로필로 실행합니다(개인 훅·전역 지침·스킬·플러그인·MCP·메모리 없음). 끄면 이 PC의 평소 CLI 설정 그대로 실행합니다. 이전 작업 컨텍스트 공유와 보안 규칙은 어느 쪽이든 같습니다. 아래 에이전트 CLI의 설치·로그인 확인과 연결도 이 선택의 프로필로 합니다.
              <span className="mt-1 block text-2xs">
                프로필에 개인 설정이 섞였는지 검사하는 기능은 아직 앱에 없습니다. 개발자는 터미널의{" "}
                <code className="rounded bg-muted px-1 py-0.5 font-mono text-2xs text-foreground">o agents doctor</code>로 볼 수 있습니다.
              </span>
            </>
          }
          control={<Switch checked={settings.clean} onChange={(clean) => updateSettings({ clean })} labelledBy="clean-label" />}
        />
        <SettingsRow
          label="프로필 위치"
          description={
            settings.clean ? (
              agentsHome ? <Path path={agentsHome} /> : "이 화면에서는 확인할 수 없습니다."
            ) : (
              "내 설정: 각 CLI의 평소 프로필을 씁니다."
            )
          }
          control={null}
        />
        <BudgetRows api={api} />
      </Rows>
      <div>
        <div className="flex items-end justify-between gap-3">
          <SubHeading title="에이전트 CLI" description="하나 이상 연결되어 있어야 총괄이 작업카드를 맡길 수 있습니다." />
          <div className="flex shrink-0 gap-2 pb-1">
            <button type="button" onClick={onOnboarding} className={BUTTON_SECONDARY}>
              연결 화면 열기
            </button>
            <button type="button" onClick={() => void agents.recheck()} disabled={agents.checking} className={BUTTON_SECONDARY}>
              <RefreshCw className={`size-3.5 ${agents.checking ? "animate-spin motion-reduce:animate-none" : ""}`} aria-hidden="true" />
              {agents.checking ? "확인하는 중" : "다시 확인"}
            </button>
          </div>
        </div>
        <div className="mt-3">
          <AgentList state={agents} />
        </div>
      </div>
    </Section>
  );
}

/**
 * 하루 사용 예산 (ADR-0004) and its switch (ADR-0013): kept by the machine in O's settings.json,
 * not in this window's storage, because `o dispatch` and the runner read them too. The machine
 * checks the values again. Off, the percent stays saved but has no effect, so its choice is inert.
 */
function BudgetRows({ api }: { api: Backend }) {
  const [report, setReport] = useState<{ enabled: boolean; percent: number }>();
  const [saving, setSaving] = useState(false);
  const [note, setNote] = useState("");
  const [unread, setUnread] = useState(false);
  const [attempt, setAttempt] = useState(0);
  // An older `o` without the switch reports no `enabled`: it is on.
  const keep = (next: BudgetReport) => setReport({ enabled: next.enabled !== false, percent: next.percent });
  useEffect(() => {
    let live = true;
    api.budget().then(
      (next) => {
        if (!live) return;
        keep(next);
        setUnread(false);
        setNote("");
      },
      () => {
        if (!live) return;
        setUnread(true);
        setNote("지금 값을 읽지 못했습니다.");
      },
    );
    return () => {
      live = false;
    };
  }, [api, attempt]);
  const save = async (change: Promise<BudgetReport>) => {
    setSaving(true);
    try {
      keep(await change);
      setNote("");
    } catch (e) {
      setNote(`저장하지 못했습니다: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setSaving(false);
    }
  };
  const enabled = report?.enabled;
  const percent = report?.percent;
  return (
    <>
      <SettingsRow
        id="budget-label"
        label="하루 사용 예산"
        description={
          <>
            {enabled === false
              ? "꺼져 있습니다. 예산으로 새 작업을 막거나 작업카드의 effort를 낮추지 않습니다. 오늘 쓴 양은 사용량의 오늘 고리에 그대로 보입니다."
              : "에이전트마다 하루에 쓸 양을 아래 비율로 잡습니다. 하루가 지나는 속도보다 빨리 쓰면 작업카드의 effort를 한두 단계 낮추고(올리지는 않음), 다 쓰면 다음 날(또는 주간 초기화)까지 그 에이전트로 새 작업을 보내지 않습니다."}{" "}
            구독 자체의 사용 한도·쿨다운과 동시 실행·하루 실행 상한은 이 선택과 관계없이 그대로입니다.
            {note && (
              <span role="status" className="mt-1 flex flex-wrap items-center gap-2 text-destructive">
                {note}
                {unread && (
                  <button type="button" onClick={() => setAttempt((n) => n + 1)} className="rounded-sm text-foreground underline underline-offset-2 hover:no-underline focus-ring">
                    다시 읽기
                  </button>
                )}
              </span>
            )}
          </>
        }
        control={<Switch checked={enabled ?? true} onChange={(next) => void save(api.setBudgetEnabled(next))} labelledBy="budget-label" disabled={enabled === undefined || saving} />}
      />
      <SettingsRow
        label="하루 예산 비율"
        description={
          enabled === false
            ? `하루 사용 예산을 켜면 주간 한도의 ${percent ?? 13}%를 하루 몫으로 씁니다.`
            : `에이전트마다 하루에 주간 한도의 ${percent ?? 13}%까지 씁니다. 사용량 기록이 없는 에이전트는 조절하지 않습니다.`
        }
        control={
          <Segmented
            label="하루 예산 비율"
            value={String(percent ?? "")}
            disabled={enabled === false}
            onChange={percent === undefined || saving || enabled === false ? undefined : (value) => void save(api.setBudgetPercent(Number(value) as BudgetPercent))}
            options={BUDGET_PERCENTS.map((p) => ({ value: String(p), label: `${p}%` }))}
          />
        }
      />
    </>
  );
}
