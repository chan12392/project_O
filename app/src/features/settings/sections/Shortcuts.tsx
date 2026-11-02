import { Keyboard } from "lucide-react";

import { Section, type SettingsSectionDef } from "../Section.tsx";

export const SHORTCUTS_SECTION = { id: "shortcuts", title: "단축키", icon: Keyboard, render: () => <ShortcutsSection /> } as const satisfies SettingsSectionDef;

const MAC = typeof navigator !== "undefined" && navigator.userAgent.includes("Mac");

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
