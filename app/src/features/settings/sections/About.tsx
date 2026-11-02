import { Info } from "lucide-react";
import type { ReactNode } from "react";

import type { Backend } from "../../../platform/backend/index.ts";
import { UpdateSettings } from "../../updates/UpdateSettings.tsx";
import { Path, Section, type AboutState, type SectionContext, type SettingsSectionDef } from "../Section.tsx";

export const ABOUT_SECTION = {
  id: "about",
  title: "정보",
  icon: Info,
  render: ({ api, about, source }) => <AboutSection api={api} about={about} source={source} />,
} as const satisfies SettingsSectionDef;

function AboutSection({ api, about, source }: { api: Backend; about: AboutState; source: SectionContext["source"] }) {
  const value = (text: string | undefined) => (about.error ? "확인할 수 없습니다" : text === undefined ? "확인하는 중" : text || "이 화면에는 없습니다");
  const sourceWord = source === "live" ? "실시간 · 이 PC의 O 실행 기록" : source === "demo" ? "데모 · 기록 재생" : "확인하는 중";
  const kindWord = api.kind === "tauri" ? "데스크톱 앱" : api.kind === "dev" ? "개발 서버" : "미리보기";
  // The app and the O CLI are versioned apart; say which number is which.
  const version = about.data?.cliVersion
    ? `앱 ${about.data.version} · O CLI ${about.data.cliVersion}`
    : api.kind === "dev" && about.data?.version
      ? `O CLI ${about.data.version}`
      : about.data?.version;
  const info: [string, ReactNode][] = [
    ["버전", value(version)],
    ["커밋", about.data?.commit || "로컬 빌드"],
    ["빌드 날짜", about.data?.builtAt ? new Date(about.data.builtAt).toLocaleString("ko-KR") : "로컬 빌드"],
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
      <UpdateSettings api={api} />
    </Section>
  );
}
