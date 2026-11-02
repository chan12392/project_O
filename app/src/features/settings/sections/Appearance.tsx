import { Palette } from "lucide-react";

import { updateSettings, useReducedMotion, useSettings, type FontSize, type Motion, type Theme } from "../../../data/settings.ts";
import { Segmented, SettingsRow } from "../../../ui/controls.tsx";
import { Rows, Section, type SettingsSectionDef } from "../Section.tsx";

export const APPEARANCE_SECTION = { id: "appearance", title: "모양", icon: Palette, render: () => <AppearanceSection /> } as const satisfies SettingsSectionDef;

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
