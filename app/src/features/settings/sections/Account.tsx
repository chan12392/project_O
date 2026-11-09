import { UserRound } from "lucide-react";

import { Section, type SettingsSectionDef } from "../Section.tsx";

export const ACCOUNT_SECTION = { id: "account", title: "계정", icon: UserRound, render: () => <AccountSection /> } as const satisfies SettingsSectionDef;

/** No sign-in exists yet, so none is offered: the app works as this PC's profile. */
function AccountSection() {
  return (
    <Section title="계정" description="Project O는 이 PC 안에서만 동작하고, 앱에 로그인하지 않습니다. 에이전트 CLI의 로그인은 에이전트 항목에서 따로 관리합니다.">
      <div className="flex items-center gap-4 rounded-xl border px-5 py-4">
        <span className="flex size-12 shrink-0 items-center justify-center rounded-full bg-muted text-muted-foreground" aria-hidden="true">
          <UserRound className="size-6" />
        </span>
        <div className="min-w-0">
          <p className="text-[15px] font-semibold">이 PC 프로필</p>
          <p className="text-xs text-muted-foreground">설정과 실행 기록은 이 PC에만 저장됩니다</p>
        </div>
      </div>
    </Section>
  );
}
