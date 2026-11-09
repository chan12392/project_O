import { Bell } from "lucide-react";
import { useState } from "react";

import { updateSettings, useSettings } from "../../../data/settings.ts";
import { BUTTON_SECONDARY, SettingsRow, Switch } from "../../../ui/controls.tsx";
import { Rows, Section, type SettingsSectionDef } from "../Section.tsx";

export const NOTIFICATIONS_SECTION = { id: "notifications", title: "알림", icon: Bell, render: () => <NotificationsSection /> } as const satisfies SettingsSectionDef;

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
              어느 작업이든 카드가 끝나거나 실패하면, 창이 뒤에 있거나 최소화되어 있을 때 데스크톱 알림을 띄웁니다. 최소화 중에는 15초마다 확인합니다. {state}
              {note && (
                <span role="status" className="mt-1 block text-destructive">
                  {note}
                </span>
              )}
            </>
          }
          control={<Switch checked={on} onChange={(next) => void toggle(next)} labelledBy="notify-label" disabled={permission === "unsupported" || permission === "denied"} />}
        />
        <SettingsRow
          id="notify-questions-label"
          label="질문 알림"
          description="총괄이나 카드가 선택을 묻거나 질문을 보내거나, 총괄이 승인을 요청하면, 창이 최소화되었거나 트레이에 있을 때 알림을 띄우고 작업 표시줄 단추를 깜빡입니다. 알림 권한이 없으면 처음 질문이 올 때 묻습니다."
          control={<Switch checked={settings.notifyQuestions} onChange={(next) => updateSettings({ notifyQuestions: next })} labelledBy="notify-questions-label" />}
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
