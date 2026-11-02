import { ArrowRight, RefreshCw, ShieldCheck } from "lucide-react";
import { useEffect, useRef } from "react";

import type { Backend } from "../backend.ts";
import { useSettings } from "../settings.ts";
import { AgentList, useAgents } from "./AgentSetup.tsx";
import { BUTTON_GHOST, BUTTON_PRIMARY, BUTTON_SECONDARY } from "./controls.tsx";

/**
 * First launch (and Settings → 에이전트): connect at least one agent CLI before
 * the lead chat. Skipping is always allowed; the chat works without agents too.
 */
export function Onboarding({ api, onDone, onSettings }: { api: Backend; onDone: () => void; onSettings: () => void }) {
  const { clean } = useSettings();
  const agents = useAgents(api, clean);
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => heading.current?.focus(), []);

  return (
    <main className="scrollbar-sleek min-h-0 min-w-0 overflow-y-auto bg-chat-canvas" aria-labelledby="onboarding-title">
      <div className="bloom mx-auto flex w-full max-w-2xl flex-col px-8 pt-16 pb-12">
        <h1 id="onboarding-title" ref={heading} tabIndex={-1} className="text-2xl font-semibold tracking-[-0.01em] text-balance focus:outline-none">
          일할 에이전트를 연결하세요
        </h1>
        <p className="mt-2 max-w-[60ch] text-sm leading-relaxed text-muted-foreground">
          총괄은 이 PC에 설치된 공식 CLI에 작업카드를 맡깁니다. 하나만 연결해도 시작할 수 있고, 나머지는 나중에 설정에서 연결하면 됩니다.
        </p>

        <div className="mt-8 flex items-center justify-between gap-3">
          <h2 className="text-sm font-semibold">에이전트 CLI</h2>
          <button type="button" onClick={() => void agents.recheck()} disabled={agents.checking} className={BUTTON_SECONDARY}>
            <RefreshCw className={`size-3.5 ${agents.checking ? "animate-spin motion-reduce:animate-none" : ""}`} aria-hidden="true" />
            {agents.checking ? "확인하는 중" : "다시 확인"}
          </button>
        </div>
        <div className="mt-3">
          <AgentList state={agents} />
        </div>

        <p className="mt-4 flex items-start gap-2 text-xs leading-relaxed text-muted-foreground">
          <ShieldCheck className="mt-px size-3.5 shrink-0" aria-hidden="true" />
          <span>
            {clean
              ? "깨끗한 프로필로 확인합니다. Project O의 에이전트는 개인 훅·전역 지침·스킬·MCP 없이 실행되어, 평소 쓰는 설정과 섞이지 않습니다."
              : "평소 로그인과 개인 설정 그대로 확인합니다. 개인 훅·전역 지침·스킬·MCP가 함께 실행됩니다."}{" "}
            <button type="button" onClick={onSettings} className="rounded underline underline-offset-2 hover:text-foreground focus-ring">
              설정에서 바꾸기
            </button>
          </span>
        </p>

        <div className="mt-10 flex items-center justify-end gap-2 border-t pt-5">
          <button type="button" onClick={onDone} className={BUTTON_GHOST}>
            나중에 할게요
          </button>
          <button type="button" onClick={onDone} disabled={!agents.ready} className={BUTTON_PRIMARY} aria-describedby={agents.ready ? undefined : "onboarding-start-hint"}>
            시작하기
            <ArrowRight className="size-3.5" aria-hidden="true" />
          </button>
        </div>
        {!agents.ready && (
          <p id="onboarding-start-hint" className="mt-2 text-right text-2xs text-muted-foreground">
            에이전트를 하나 이상 연결하면 시작할 수 있습니다.
          </p>
        )}
      </div>
    </main>
  );
}
