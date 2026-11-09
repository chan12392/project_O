import { Bot, RefreshCw } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { updateSettings, useSettings } from "../../../data/settings.ts";
import { BUDGET_PERCENTS, type AgentCatalogEntry, type Backend, type BudgetPercent, type BillingMode, type BillingReport, type BudgetReport, type RamChange, type RamReport, type YoloChange, type YoloReport } from "../../../platform/backend/index.ts";
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
  const billing = useBilling(api);
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
        <RamRows api={api} />
        <BillingRow billing={billing} />
        <YoloRows api={api} />
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
          <AgentList state={agents} showApiKeyWarning billingMode={billing.report?.mode} />
        </div>
      </div>
      <AgentCatalog api={api} />
    </Section>
  );
}

const catalogCache = new WeakMap<Backend, AgentCatalogEntry[]>();

function AgentCatalog({ api }: { api: Backend }) {
  const [entries, setEntries] = useState<AgentCatalogEntry[]>(() => catalogCache.get(api) ?? []);
  const [error, setError] = useState("");
  const [checking, setChecking] = useState(false);
  const generation = useRef(0);
  useEffect(() => {
    setEntries(catalogCache.get(api) ?? []);
    if (!catalogCache.has(api)) void detect(false);
    return () => { generation.current++; };
  }, [api]);
  function detect(refresh: boolean) {
    const request = ++generation.current;
    setChecking(true);
    setError("");
    return api.agentsCatalog(refresh).then((list) => {
      if (request !== generation.current) return;
      catalogCache.set(api, list);
      setEntries(list);
    }, (e) => { if (request === generation.current) setError(String(e)); }).finally(() => { if (request === generation.current) setChecking(false); });
  }
  return <div>
    <div className="flex items-end justify-between gap-3">
      <SubHeading title="다른 에이전트 CLI" description="설치 여부와 연결 지원 상태를 확인하세요." />
      <button type="button" onClick={() => void detect(true)} disabled={checking} className={BUTTON_SECONDARY}>
        <RefreshCw className={`size-3.5 ${checking ? "animate-spin motion-reduce:animate-none" : ""}`} aria-hidden="true" />
        {checking ? "감지하는 중" : "다시 감지"}
      </button>
    </div>
    {error && <p role="alert" className="text-sm text-destructive">목록을 읽지 못했습니다: {error}</p>}
    {checking && entries.length === 0 && <p role="status" className="text-sm text-muted-foreground">감지하는 중입니다</p>}
    {entries.length > 0 && <ul className="mt-3 divide-y rounded-lg border">{entries.map((entry) => <li key={entry.id} className="flex flex-wrap items-start justify-between gap-3 px-4 py-3">
      <div><p className="text-sm font-medium">{entry.name}</p><p className="text-xs text-muted-foreground">{entry.detected ? entry.version ?? "감지됨" : "설치되지 않음"} · {entry.status === "connected" ? "연결 지원" : entry.status === "planned" ? "연결 예정" : "지원하지 않음"}{entry.profileSupported ? "" : " · 프로필 미지원"}</p><p className="mt-1 text-xs text-muted-foreground">{entry.note}</p></div>
      {entry.installUrl && <button type="button" className={BUTTON_SECONDARY} onClick={() => void api.openExternal(entry.installUrl!)}>공식 설치 안내</button>}
    </li>)}</ul>}
  </div>;
}

const RAM_PERCENTS = [50, 60, 70, 80, 90] as const;

/**
 * RAM 상한 (ADR-0037): kept in O's settings.json like the budget, because the runner reads it
 * when a card is about to start. A saved percent outside the buttons (`o ram --percent 75`) shows unselected.
 */
function RamRows({ api }: { api: Backend }) {
  const [report, setReport] = useState<RamReport>();
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => { void api.ram().then(setReport, (e) => setError(`지금 값을 읽지 못했습니다: ${String(e)}`)); }, [api]);
  const save = async (change: RamChange) => {
    setBusy(true);
    try { setReport(await api.setRam(change)); setError(""); }
    catch (e) { setError(`저장하지 못했습니다: ${String(e)}`); }
    finally { setBusy(false); }
  };
  const used = report?.used == null ? "이 PC에서는 사용률을 읽지 못해 상한이 적용되지 않습니다." : `지금 사용률 ${report.used}%.`;
  return <>
    <SettingsRow id="ram-label" label="RAM 상한" description={<>
      {report?.enabled === false
        ? "꺼져 있습니다. 메모리 사용률과 관계없이 동시 실행 상한까지 작업카드를 시작합니다."
        : `메모리 사용률이 ${report?.percent ?? 70}% 이상이면 새 작업카드를 시작하지 않고 차례를 기다리게 합니다. 실행 중인 카드가 하나도 없으면 시작하고, 이미 실행 중인 카드는 끝내지 않습니다.`}{" "}
      {report && used}
      {error && <span role="alert" className="block text-destructive">{error}</span>}
    </>} control={<Switch checked={report?.enabled ?? true} disabled={!report || busy} labelledBy="ram-label" onChange={(enabled) => void save({ enabled })} />} />
    <SettingsRow label="RAM 상한 비율" description="작업카드를 더 시작하지 않는 메모리 사용률입니다." control={
      <Segmented
        label="RAM 상한 비율"
        value={String(report?.percent ?? "")}
        disabled={report?.enabled === false}
        onChange={!report || busy || !report.enabled ? undefined : (value) => void save({ percent: Number(value) })}
        options={RAM_PERCENTS.map((p) => ({ value: String(p), label: `${p}%` }))}
      />
    } />
  </>;
}

/**
 * API 과금 (ADR-0035 개정 1): kept in O's settings.json, because the runner, the lead and reviews
 * read it before they start a CLI. Held here so the agent list's API-key warning follows the mode.
 */
function useBilling(api: Backend) {
  const [report, setReport] = useState<BillingReport>();
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => { void api.billing().then(setReport, (e) => setError(`지금 값을 읽지 못했습니다: ${String(e)}`)); }, [api]);
  const save = async (mode: BillingMode) => {
    setBusy(true);
    try { setReport(await api.setBilling(mode)); setError(""); }
    catch (e) { setError(`저장하지 못했습니다: ${String(e)}`); }
    finally { setBusy(false); }
  };
  return { report, error, busy, save };
}

function BillingRow({ billing: { report, error, busy, save } }: { billing: ReturnType<typeof useBilling> }) {
  return <SettingsRow label="API 과금" description={<>
    {report?.mode === "subscription"
      ? "구독만: 그 CLI가 쓰는 API 키 환경변수가 있으면 작업카드·총괄·검토를 시작하지 않고 변수 이름을 알려 줍니다."
      : "경고만: API 키 환경변수가 있어도 그대로 전달하고 경고만 표시합니다. 구독 대신 API 요금이 나갈 수 있습니다."}{" "}
    O는 변수를 지우거나 값을 읽지 않습니다.
    {error && <span role="alert" className="block text-destructive">{error}</span>}
  </>} control={
    <Segmented
      label="API 과금"
      value={report?.mode ?? ""}
      onChange={!report || busy ? undefined : (mode) => { if (mode !== report.mode) void save(mode as BillingMode); }}
      options={[{ value: "warn", label: "경고만" }, { value: "subscription", label: "구독만" }]}
    />
  } />;
}

function YoloRows({ api }: { api: Backend }) {
  const [report, setReport] = useState<YoloReport>();
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => { void api.yolo().then(setReport, (e) => setError(String(e))); }, [api]);
  const save = async (change: YoloChange) => {
    setBusy(true);
    try { setReport(await api.setYolo(change)); setError(""); }
    catch (e) { setError(`저장하지 못했습니다: ${String(e)}`); }
    finally { setBusy(false); }
  };
  return <>
    <SettingsRow id="yolo-label" label="YOLO 모드" description={<>
      총괄과 작업자가 승인 없이 파일을 고치고 명령을 실행합니다. 앱에서 제안된 계획은 작업 분배를 눌러야 시작합니다.
      같은 Windows 사용자 권한에서는 에이전트가 실행 표식을 지우거나 별도 프로세스를 이용해 직접 전달 제한을 우회할 수 있습니다.
      {error && <span role="alert" className="block text-destructive">{error}</span>}
    </>} control={<Switch checked={report?.enabled ?? false} disabled={!report || busy} labelledBy="yolo-label" onChange={(enabled) => {
      if (!enabled || window.confirm("YOLO를 켜면 승인 없이 파일 수정과 명령 실행이 가능합니다. CLI에서는 조건에 따라 계획이 자동 실행될 수 있습니다. 켜시겠습니까?")) void save({ enabled });
    }} />} />
    {report && ([
      ["maxCards", "자동 실행 카드 수", 1, 6],
      ["perHour", "시간당 자동 실행 횟수", 1, 6],
      ["dedupeMinutes", "같은 계획 대기 시간(분)", 10, 120],
    ] as const).map(([key, label, min, max]) => <SettingsRow key={key} label={label} description={`${min}–${max} 사이 정수`} control={
      <input aria-label={label} type="number" min={min} max={max} step={1} key={`${key}-${report.autoRun[key]}`} defaultValue={report.autoRun[key]} disabled={busy}
        className="w-20 rounded border bg-background px-2 py-1 text-sm focus-ring" onBlur={(e) => {
          const value = e.currentTarget.valueAsNumber;
          if (!Number.isInteger(value) || value < min || value > max) { setError(`${label}: ${min}–${max} 사이 정수를 입력하세요`); e.currentTarget.value = String(report.autoRun[key]); return; }
          if (value !== report.autoRun[key]) void save({ [key]: value });
        }} />
    } />)}
  </>;
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
