import { LogIn, UserRound } from "lucide-react";

import { BUTTON_SECONDARY, SoonBadge, SubHeading } from "../../../ui/controls.tsx";
import { Section, type SettingsSectionDef } from "../Section.tsx";

export const ACCOUNT_SECTION = { id: "account", title: "계정", icon: UserRound, render: () => <AccountSection /> } as const satisfies SettingsSectionDef;

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
