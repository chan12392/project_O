import { ArrowLeft, Bell, Bot, Info, Keyboard, LogIn, Palette, RefreshCw, SlidersHorizontal, UserRound, type LucideIcon } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";

import { BUDGET_PERCENTS, type Backend, type BudgetPercent } from "../backend.ts";
import { sessionsShown, updateSettings, useReducedMotion, useSettings, type FontSize, type Motion, type StartScreen, type Theme } from "../settings.ts";
import { AgentList, useAgents } from "./AgentSetup.tsx";
import { BUTTON_SECONDARY, Segmented, SettingsRow, SoonBadge, SubHeading, Switch } from "./controls.tsx";

export type SettingsSection = "account" | "general" | "appearance" | "agents" | "notifications" | "shortcuts" | "about";

interface NavItem {
  id: SettingsSection;
  title: string;
  icon: LucideIcon;
}

/** Settings navigation: groups of sections. */
const GROUPS: { title: string; items: NavItem[] }[] = [
  {
    title: "기본",
    items: [
      { id: "account", title: "계정", icon: UserRound },
      { id: "general", title: "일반", icon: SlidersHorizontal },
      { id: "appearance", title: "모양", icon: Palette },
    ],
  },
  {
    title: "작업",
    items: [
      { id: "agents", title: "에이전트", icon: Bot },
      { id: "notifications", title: "알림", icon: Bell },
    ],
  },
  {
    title: "도움말",
    items: [
      { id: "shortcuts", title: "단축키", icon: Keyboard },
      { id: "about", title: "정보", icon: Info },
    ],
  },
];

const MAC = typeof navigator !== "undefined" && navigator.userAgent.includes("Mac");

interface Props {
  api: Backend;
  section: SettingsSection;
  /** Where the screen's runs come from, for 정보. */
  source: "live" | "demo" | "pending";
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
          {GROUPS.map((group) => (
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
          {section === "account" && <AccountSection />}
          {section === "general" && <GeneralSection demo={source === "demo"} />}
          {section === "appearance" && <AppearanceSection />}
          {section === "agents" && <AgentsSection api={api} agentsHome={about.data?.agentsHome} onOnboarding={onOnboarding} />}
          {section === "notifications" && <NotificationsSection />}
          {section === "shortcuts" && <ShortcutsSection />}
          {section === "about" && <AboutSection api={api} about={about} source={source} />}
        </div>
      </div>
    </div>
  );
}

function Section({ title, description, children }: { title: string; description: string; children: ReactNode }) {
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
function Rows({ children }: { children: ReactNode }) {
  return <div className="divide-y">{children}</div>;
}

type About = Awaited<ReturnType<Backend["about"]>>;

function Path({ path }: { path: string }) {
  return <span className="font-mono text-2xs break-all text-foreground/85">{path}</span>;
}

function useAbout(api: Backend) {
  const [state, setState] = useState<{ data?: About; error?: string }>({});
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

function AccountSection() {
  return (
    <Section title="계정" description="Project O는 지금 이 PC 안에서만 동작합니다. 계정 없이도 모든 기능을 쓸 수 있습니다.">
      <div className="flex items-center gap-4 rounded-xl border px-5 py-4">
        <span className="flex size-12 shrink-0 items-center justify-center rounded-full bg-muted text-muted-foreground" aria-hidden="true">
          <UserRound className="size-6" />
        </span>
        <div className="min-w-0">
          <p className="text-[15px] font-semibold">로그인하지 않음</p>
          <p className="text-xs text-muted-foreground">이 PC에만 있는 로컬 프로필 · 설정은 이 PC에 저장됩니다</p>
        </div>
      </div>
      <div>
        <SubHeading title="로그인" description="여러 PC에서 같은 프로필을 쓰기 위한 로그인입니다. 에이전트 CLI의 로그인과는 별개입니다." />
        <div className="mt-3 flex flex-wrap gap-2">
          {["Google", "카카오", "Apple"].map((name) => (
            <button key={name} type="button" disabled className={BUTTON_SECONDARY}>
              <LogIn className="size-3.5" aria-hidden="true" />
              {name}로 계속하기
              <SoonBadge />
            </button>
          ))}
        </div>
      </div>
    </Section>
  );
}

function GeneralSection({ demo }: { demo: boolean }) {
  const settings = useSettings();
  const sessions = sessionsShown(settings, demo);
  return (
    <Section title="일반" description="앱을 열 때의 화면과 실행 기록을 읽는 방식입니다.">
      <Rows>
        <SettingsRow
          id="sessions-label"
          label="Claude Code 세션도 보기"
          description={
            demo
              ? "미리보기에서는 예시 세션 하나(블로그 프로젝트)를 보여 줍니다. 메인 대화가 총괄 자리에, 서브에이전트가 사무실의 사람으로 나옵니다."
              : "O로 보내지 않고 Claude Code에서 바로 한 작업도 최근 24시간 것을 읽기 전용으로 불러옵니다. 메인 대화는 총괄 자리에, 서브에이전트는 사무실의 사람으로 보이고 다음 할 일에도 잡힙니다. 대화 기록 폴더(projects)만 읽고 로그인 정보는 읽지 않습니다."
          }
          control={<Switch checked={sessions} onChange={(next) => updateSettings({ sessions: next })} labelledBy="sessions-label" />}
        />
        <SettingsRow
          label="시작 화면"
          description={
            settings.start === "last"
              ? "앱을 열면 지난번에 보던 프로젝트의 총괄 대화로 갑니다."
              : "앱을 열 때마다 에이전트 연결 화면부터 보여 줍니다."
          }
          control={
            <Segmented<StartScreen>
              label="시작 화면"
              value={settings.start}
              onChange={(start) => updateSettings({ start })}
              options={[
                { value: "last", label: "마지막 프로젝트" },
                { value: "onboarding", label: "에이전트 연결 화면" },
              ]}
            />
          }
        />
        <SettingsRow
          label="실시간 갱신 주기"
          soon
          description="이 PC의 O 실행 기록을 2초마다 읽고, 창이 가려져 있으면 쉽니다. 주기 바꾸기는 준비 중입니다."
          control={
            <Segmented
              label="실시간 갱신 주기"
              value="2"
              options={[
                { value: "2", label: "2초" },
                { value: "5", label: "5초", soon: true },
                { value: "10", label: "10초", soon: true },
              ]}
            />
          }
        />
      </Rows>
    </Section>
  );
}

function AppearanceSection() {
  const settings = useSettings();
  const reduced = useReducedMotion();
  return (
    <Section title="모양" description="이 PC에서 보는 화면의 테마, 글자 크기, 움직임입니다.">
      <Rows>
        <SettingsRow
          label="테마"
          description="시스템을 고르면 Windows의 밝게·어둡게 설정을 따라갑니다."
          control={
            <Segmented<Theme>
              label="테마"
              value={settings.theme}
              onChange={(theme) => updateSettings({ theme })}
              options={[
                { value: "system", label: "시스템" },
                { value: "light", label: "라이트" },
                { value: "dark", label: "다크" },
              ]}
            />
          }
        />
        <SettingsRow
          label="글자 크기"
          description="화면 전체의 글자와 간격이 함께 커지거나 작아집니다."
          control={
            <Segmented<FontSize>
              label="글자 크기"
              value={settings.fontSize}
              onChange={(fontSize) => updateSettings({ fontSize })}
              options={[
                { value: "small", label: "작게" },
                { value: "normal", label: "기본" },
                { value: "large", label: "크게" },
              ]}
            />
          }
        />
        <SettingsRow
          label="움직임 줄이기"
          description={`사무실 사람들의 걷기·손짓과 화면 전환 효과입니다. 지금은 ${reduced ? "멈춰 있습니다" : "움직입니다"}.`}
          control={
            <Segmented<Motion>
              label="움직임 줄이기"
              value={settings.motion}
              onChange={(motion) => updateSettings({ motion })}
              options={[
                { value: "system", label: "시스템 따르기" },
                { value: "reduce", label: "줄이기" },
                { value: "full", label: "줄이지 않기" },
              ]}
            />
          }
        />
      </Rows>
    </Section>
  );
}

function AgentsSection({ api, agentsHome, onOnboarding }: { api: Backend; agentsHome?: string; onOnboarding: () => void }) {
  const settings = useSettings();
  const agents = useAgents(api, settings.clean);
  return (
    <Section title="에이전트" description="작업카드를 맡을 공식 CLI의 설치·연결 상태와 실행 프로필입니다.">
      <Rows>
        <SettingsRow
          id="clean-label"
          label="깨끗한 에이전트로 실행"
          description={
            <>
              개인 훅·전역 지침·스킬·플러그인·MCP·메모리 없이 공식 CLI만으로 실행합니다. 평소 쓰는 설정은 건드리지 않습니다. 상태 확인과 연결에 쓰는 프로필이 이 선택을 따릅니다. 터미널에서 확인:{" "}
              <code className="rounded bg-muted px-1 py-0.5 font-mono text-2xs text-foreground">o agents doctor</code>
            </>
          }
          control={<Switch checked={settings.clean} onChange={(clean) => updateSettings({ clean })} labelledBy="clean-label" />}
        />
        <BudgetRow api={api} />
        <SettingsRow
          label="프로필 위치"
          description={
            settings.clean ? (
              agentsHome ? <Path path={agentsHome} /> : "이 화면에서는 확인할 수 없습니다."
            ) : (
              "깨끗한 실행을 끄면 각 CLI의 평소 프로필을 씁니다."
            )
          }
          control={null}
        />
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
 * 하루 사용 예산 (ADR-0004): kept by the machine in O's settings.json, not in this
 * window's storage, because `o dispatch` reads it too. The machine checks the range again.
 */
function BudgetRow({ api }: { api: Backend }) {
  const [percent, setPercent] = useState<number>();
  const [saving, setSaving] = useState(false);
  const [note, setNote] = useState("");
  useEffect(() => {
    let live = true;
    api.budget().then(
      (report) => live && setPercent(report.percent),
      () => live && setNote("지금 값을 읽지 못했습니다. 터미널에서 o budget으로 확인할 수 있습니다."),
    );
    return () => {
      live = false;
    };
  }, [api]);
  const choose = async (value: string) => {
    const next = Number(value) as BudgetPercent;
    setSaving(true);
    try {
      setPercent((await api.setBudgetPercent(next)).percent);
      setNote("");
    } catch (e) {
      setNote(`저장하지 못했습니다: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setSaving(false);
    }
  };
  return (
    <SettingsRow
      label="하루 사용 예산"
      description={
        <>
          에이전트마다 하루에 쓸 양을 주간 한도의 {percent ?? 13}%로 잡습니다. 하루가 지나는 속도보다 빨리 쓰면 작업카드의 effort를 한두 단계 낮추고(올리지는 않음), 다 쓰면 다음 날(또는 주간 초기화)까지 그 에이전트로 새 작업을 보내지 않습니다. 사용량 기록이 없는 에이전트는 조절하지 않습니다.
          {note && (
            <span role="status" className="mt-1 block text-destructive">
              {note}
            </span>
          )}
        </>
      }
      control={
        <Segmented
          label="하루 사용 예산"
          value={String(percent ?? "")}
          onChange={percent === undefined || saving ? undefined : (value) => void choose(value)}
          options={BUDGET_PERCENTS.map((p) => ({ value: String(p), label: `${p}%` }))}
        />
      }
    />
  );
}

type Permission = NotificationPermission | "unsupported";

function NotificationsSection() {
  const settings = useSettings();
  const supported = typeof Notification !== "undefined";
  const [permission, setPermission] = useState<Permission>(supported ? Notification.permission : "unsupported");
  const [note, setNote] = useState("");
  const on = settings.notify && permission === "granted";

  const toggle = async (next: boolean) => {
    if (!next) return updateSettings({ notify: false });
    const result = permission === "granted" ? permission : await Notification.requestPermission();
    setPermission(result);
    updateSettings({ notify: result === "granted" });
    setNote(result === "granted" ? "" : "알림이 허용되지 않아 켜지 않았습니다.");
  };
  const test = () => {
    try {
      new Notification("카드 완료 알림", { body: "이렇게 알려 드립니다. 실제 알림은 창을 보고 있지 않을 때만 뜹니다." });
      setNote("");
    } catch {
      setNote("이 화면에서는 알림을 보낼 수 없습니다.");
    }
  };

  const state =
    permission === "unsupported"
      ? "이 화면에서는 알림을 쓸 수 없습니다."
      : permission === "denied"
        ? "알림이 차단되어 있습니다. Windows 또는 브라우저의 알림 설정에서 허용한 뒤 다시 켜 주세요."
        : permission === "default"
          ? "켜면 알림 권한을 묻습니다."
          : "알림 권한이 허용되어 있습니다.";

  return (
    <Section title="알림" description="창을 보고 있지 않을 때 알아야 할 일을 알려 드립니다.">
      <Rows>
        <SettingsRow
          id="notify-label"
          label="카드 완료 알림"
          description={
            <>
              보고 있는 작업의 카드가 끝나거나 실패하면, 창이 뒤에 있을 때 데스크톱 알림을 띄웁니다. {state}
              {note && (
                <span role="status" className="mt-1 block text-destructive">
                  {note}
                </span>
              )}
            </>
          }
          control={<Switch checked={on} onChange={(next) => void toggle(next)} labelledBy="notify-label" disabled={permission === "unsupported" || permission === "denied"} />}
        />
        {on && (
          <SettingsRow
            label="알림 확인"
            description="지금 알림 하나를 보내 봅니다."
            control={
              <button type="button" onClick={test} className={BUTTON_SECONDARY}>
                테스트 알림 보내기
              </button>
            }
          />
        )}
      </Rows>
    </Section>
  );
}

/** Only shortcuts the app really has (Composer, dismiss.ts, the panel separator, group names). */
const SHORTCUTS: { keys: string[][]; what: string; where: string }[] = [
  { keys: [[MAC ? "⌘" : "Ctrl", "Enter"]], what: "총괄에게 요청 보내기", where: "요청 입력창" },
  { keys: [["Esc"]], what: "메뉴 닫기, 열었던 버튼으로 돌아가기", where: "계정 메뉴, 프로젝트 ⋯ 메뉴, 일하는 사람 목록" },
  { keys: [["Enter"], ["Esc"]], what: "그룹 이름 저장 / 취소", where: "그룹 이름 입력" },
  { keys: [["←"], ["→"]], what: "오른쪽 패널 너비 조절", where: "채팅과 패널 사이 구분선" },
  { keys: [["Shift", "←"], ["Shift", "→"]], what: "패널 너비 크게 조절", where: "채팅과 패널 사이 구분선" },
  { keys: [["Tab"], ["Shift", "Tab"]], what: "다음 / 이전 컨트롤로 이동", where: "어디서나" },
];

function Keys({ combo }: { combo: string[] }) {
  return (
    <span className="inline-flex items-center gap-0.5">
      {combo.map((key) => (
        <kbd key={key} className="min-w-6 rounded border bg-muted px-1.5 py-0.5 text-center font-mono text-2xs text-foreground">
          {key}
        </kbd>
      ))}
    </span>
  );
}

function ShortcutsSection() {
  return (
    <Section title="단축키" description="지금 앱에 있는 키보드 동작입니다. 단축키 바꾸기는 아직 없습니다.">
      <table className="w-full text-[13px]">
        <thead className="sr-only">
          <tr>
            <th>키</th>
            <th>동작</th>
            <th>쓰는 곳</th>
          </tr>
        </thead>
        <tbody className="divide-y">
          {SHORTCUTS.map((s) => (
            <tr key={s.what}>
              <td className="py-3 pr-4 align-top whitespace-nowrap">
                <span className="inline-flex flex-wrap items-center gap-1.5">
                  {s.keys.map((combo, i) => (
                    <span key={combo.join("+")} className="inline-flex items-center gap-1.5">
                      {i > 0 && <span className="text-2xs text-muted-foreground">/</span>}
                      <Keys combo={combo} />
                    </span>
                  ))}
                </span>
              </td>
              <td className="py-3 pr-4 align-top">{s.what}</td>
              <td className="py-3 align-top text-xs text-muted-foreground">{s.where}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <p className="text-xs text-muted-foreground">그룹 머리글을 두 번 누르면 그룹 이름을 바꿀 수 있습니다.</p>
    </Section>
  );
}

function AboutSection({ api, about, source }: { api: Backend; about: { data?: About; error?: string }; source: Props["source"] }) {
  const value = (text: string | undefined) => (about.error ? "확인할 수 없습니다" : text === undefined ? "확인하는 중" : text || "이 화면에는 없습니다");
  const sourceWord = source === "live" ? "실시간 · 이 PC의 O 실행 기록" : source === "demo" ? "데모 · 기록 재생" : "확인하는 중";
  const kindWord = api.kind === "tauri" ? "데스크톱 앱" : api.kind === "dev" ? "개발 서버" : "미리보기";
  const info: [string, ReactNode][] = [
    ["버전", value(about.data?.version)],
    ["데이터 출처", sourceWord],
    ["연결", kindWord],
    ["O 상태 폴더", about.data?.home ? <Path path={about.data.home} /> : value(about.data?.home)],
    ["에이전트 프로필 폴더", about.data?.agentsHome ? <Path path={about.data.agentsHome} /> : value(about.data?.agentsHome)],
  ];
  return (
    <Section title="정보" description="이 앱의 버전과 데이터가 있는 곳입니다.">
      <dl className="divide-y">
        {info.map(([label, text]) => (
          <div key={label} className="grid grid-cols-[10rem_minmax(0,1fr)] gap-4 py-3 text-[13px]">
            <dt className="text-muted-foreground">{label}</dt>
            <dd className="min-w-0">{text}</dd>
          </div>
        ))}
      </dl>
      {about.error && <p className="text-xs text-muted-foreground">버전과 폴더는 데스크톱 앱이나 개발 서버의 /api/about이 알려 줍니다.</p>}
    </Section>
  );
}
