import { SlidersHorizontal } from "lucide-react";

import { sessionsShown, updateSettings, useSettings, type CloseAction, type StartScreen } from "../../../data/settings.ts";
import { Segmented, SettingsRow, Switch } from "../../../ui/controls.tsx";
import { Rows, Section, type SettingsSectionDef } from "../Section.tsx";

export const GENERAL_SECTION = {
  id: "general",
  title: "일반",
  icon: SlidersHorizontal,
  render: ({ source }) => <GeneralSection demo={source === "demo"} />,
} as const satisfies SettingsSectionDef;

function GeneralSection({ demo }: { demo: boolean }) {
  const settings = useSettings();
  const sessions = sessionsShown(settings, demo);
  return (
    <Section title="일반" description="앱을 열 때의 화면과 실행 기록을 읽는 방식입니다.">
      <Rows>
        <SettingsRow
          label="창 닫기 (X)"
          description="트레이로 숨기거나 앱을 완전히 종료합니다. 작업 중인 에이전트가 있으면 종료 전에 알려 드립니다."
          control={<Segmented<CloseAction> label="창 닫기 동작" value={settings.closeAction} onChange={(closeAction) => updateSettings({ closeAction })} options={[
            { value: "ask", label: "물어보기" }, { value: "tray", label: "트레이" }, { value: "quit", label: "완전 종료" },
          ]} />}
        />
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
        {/* What the app does, not a choice it does not offer yet. */}
        <SettingsRow
          label="실시간 갱신"
          description="이 PC의 O 실행 기록을 2초마다 읽고, 창이 가려져 있으면 쉽니다."
          control={<span className="text-xs text-muted-foreground tabular-nums">2초마다</span>}
        />
      </Rows>
    </Section>
  );
}
