import { Keyboard } from "lucide-react";

import { Section, type SettingsSectionDef } from "../Section.tsx";

export const SHORTCUTS_SECTION = { id: "shortcuts", title: "단축키", icon: Keyboard, render: () => <ShortcutsSection /> } as const satisfies SettingsSectionDef;

const MENUS = "실행 프로필 메뉴, 총괄 에이전트·모델 메뉴";

/**
 * Only keys the app really handles, each where it handles them: Composer (send, model id),
 * App.tsx (Esc leaves Settings), lib/dismiss.ts (account, project ⋯ and who-is-working
 * menus), ui/Menu.tsx (the composer's menus), UpdatePrompt (its dialog's Esc is "나중에"),
 * panelLayout.ts (the separator), Sidebar and
 * ProjectForms (group name, new project, folder path). There are no global shortcuts.
 */
const SHORTCUTS: { keys: string[][]; what: string; where: string }[] = [
  { keys: [["Enter"]], what: "총괄에게 요청 보내기", where: "요청 입력창" },
  { keys: [["Ctrl", "Enter"], ["Shift", "Enter"]], what: "줄바꿈", where: "요청 입력창" },
  { keys: [["Esc"]], what: "설정을 닫고 앱으로 돌아가기", where: "설정 화면 (입력칸에 있거나 메뉴가 열려 있으면 그쪽이 먼저)" },
  { keys: [["Esc"]], what: "메뉴 닫기, 열었던 버튼으로 돌아가기", where: `계정 메뉴, 프로젝트 ⋯ 메뉴, 일하는 사람 목록, ${MENUS}` },
  { keys: [["Esc"]], what: "업데이트 안내를 나중에 보기로 닫기", where: "새 버전 안내 창 (데스크톱 앱)" },
  { keys: [["↑"], ["↓"]], what: "메뉴 열기 / 항목 사이 이동", where: MENUS },
  { keys: [["Home"], ["End"]], what: "첫 항목 / 마지막 항목으로", where: MENUS },
  { keys: [["Enter"], ["Esc"]], what: "저장·만들기·등록·적용 / 취소", where: "그룹 이름, 왼쪽 목록의 새 프로젝트 이름·등록할 폴더 경로, 총괄 모델 id 입력" },
  { keys: [["←"], ["→"]], what: "오른쪽 패널 넓히기 / 좁히기", where: "채팅과 패널 사이 구분선" },
  { keys: [["Shift", "←"], ["Shift", "→"]], what: "패널 너비 크게 조절", where: "채팅과 패널 사이 구분선" },
  { keys: [["Tab"], ["Shift", "Tab"]], what: "다음 / 이전 컨트롤로 이동 (첫 Tab에 '대화 입력으로 건너뛰기')", where: "어디서나" },
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
    <Section title="단축키" description="지금 앱에 있는 키보드 동작입니다. 어디서나 통하는 전역 단축키와 단축키 바꾸기는 아직 없습니다.">
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
            <tr key={`${s.keys.flat().join("+")} ${s.what}`}>
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
