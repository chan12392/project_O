import { Check, CircleAlert, CircleDashed, Download, ExternalLink, LogIn, RefreshCw, Terminal } from "lucide-react";
import { useEffect, useState } from "react";

import { canLead } from "../../../../src/core/lead.ts";
import { useAgentCheck } from "../../data/agents.ts";
import type { ActionResult, AgentId, AgentStatus, Backend } from "../../platform/backend/index.ts";
import { BUTTON_PRIMARY, BUTTON_SECONDARY } from "../../ui/controls.tsx";

type Busy = "install" | "login" | "terminal";

interface Note {
  ok: boolean;
  message: string;
  log?: string;
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Can lead now (installed, a lead agent, not known to be signed out): the composer's rule too. */
export const usable = (agent: AgentStatus): boolean => canLead(agent);

/**
 * The agent CLIs' install and sign-in state, through `backend()`: the check every screen
 * shares (once per profile, until "다시 확인"), run again after every action.
 */
export function useAgents(api: Backend, clean: boolean) {
  // The check itself is shared with the other screens (data/agents.ts): opening this one does not run it again.
  const { agents, error, checking, recheck } = useAgentCheck(api, clean);
  const [busy, setBusy] = useState<Partial<Record<AgentId, Busy>>>({});
  const [notes, setNotes] = useState<Partial<Record<AgentId, Note>>>({});

  useEffect(() => setNotes({}), [clean]);

  const act = async (id: AgentId, kind: Busy) => {
    setBusy((b) => ({ ...b, [id]: kind }));
    setNotes((n) => ({ ...n, [id]: undefined }));
    let result: ActionResult;
    try {
      result = kind === "install" ? await api.install(id) : kind === "terminal" ? await api.openTerminal() : await api.login(id, clean);
    } catch (e) {
      result = { ok: false, message: kind === "install" ? "설치를 시작하지 못했습니다." : kind === "terminal" ? "터미널을 열지 못했습니다." : "로그인 창을 열지 못했습니다.", log: errorText(e) };
    }
    setBusy((b) => ({ ...b, [id]: undefined }));
    setNotes((n) => ({ ...n, [id]: result }));
    await recheck();
  };

  return {
    agents,
    error,
    checking,
    busy,
    notes,
    recheck,
    install: (id: AgentId) => act(id, "install"),
    login: (id: AgentId) => act(id, "login"),
    terminal: () => act("agy", "terminal"),
    openExternal: (url: string) => void api.openExternal(url).catch(() => window.open(url, "_blank", "noopener")),
    /** At least one agent can lead (the composer can send). */
    ready: Boolean(agents?.some(usable)),
  };
}

export type AgentsState = ReturnType<typeof useAgents>;

/** Each CLI with its state and the one next step it needs: install, sign in, or nothing. */
export function AgentList({ state }: { state: AgentsState }) {
  const { agents, error, checking } = state;
  if (!agents) {
    if (error)
      return (
        <div className="rounded-lg border px-4 py-4" role="alert">
          <p className="flex items-center gap-2 text-[13px] font-medium">
            <CircleAlert className="size-4 shrink-0 text-status-warning" aria-hidden="true" />
            에이전트 상태를 읽지 못했습니다
          </p>
          <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
            상태 확인은 데스크톱 앱이나 개발 서버의 /api가 맡습니다. 잠시 뒤 다시 확인해 보세요.
          </p>
          <Details label="오류 내용" text={error} />
          <button type="button" onClick={() => void state.recheck()} disabled={checking} className={`mt-3 ${BUTTON_SECONDARY}`}>
            <RefreshCw className={`size-3.5 ${checking ? "animate-spin motion-reduce:animate-none" : ""}`} aria-hidden="true" />
            다시 확인
          </button>
        </div>
      );
    return (
      <ul className="divide-y rounded-lg border" aria-busy="true" aria-label="에이전트 상태 확인 중">
        {[0, 1, 2].map((i) => (
          <li key={i} className="flex items-center gap-3 px-4 py-3.5" aria-hidden="true">
            <span className="shimmer size-7 rounded-md" />
            <span className="flex-1 space-y-1.5">
              <span className="shimmer block h-3 w-28 rounded" />
              <span className="shimmer block h-2.5 w-44 rounded" />
            </span>
            <span className="shimmer h-8 w-16 rounded-md" />
          </li>
        ))}
      </ul>
    );
  }
  return (
    <div>
      {error && (
        <p className="mb-2 flex items-center gap-2 text-xs text-status-warning-text" role="alert">
          <CircleAlert className="size-3.5 shrink-0" aria-hidden="true" />
          다시 확인하지 못했습니다. 아래는 지난번 확인 결과입니다.
        </p>
      )}
      <ul className="divide-y rounded-lg border" aria-busy={checking}>
        {agents.map((agent) => (
          <AgentRow key={agent.id} agent={agent} busy={state.busy[agent.id]} note={state.notes[agent.id]} state={state} />
        ))}
      </ul>
    </div>
  );
}

function AgentRow({ agent, busy, note, state }: { agent: AgentStatus; busy?: Busy; note?: Note; state: AgentsState }) {
  const connected = agent.installed && !agent.profileUnsupported && agent.loggedIn === true;
  const unknown = agent.installed && !agent.profileUnsupported && agent.loggedIn === null;
  const needsLogin = agent.installed && !agent.profileUnsupported && agent.loggedIn === false;
  const version = agent.version ? ` · ${agent.version}` : "";
  const status = busy === "install"
    ? "설치하는 중입니다. 몇 분 걸릴 수 있습니다."
    : busy === "login"
      ? "로그인 창을 여는 중입니다."
      : busy === "terminal"
        ? "터미널을 여는 중입니다."
      : agent.id === "agy" && agent.installed
        ? `설치됨${version} · 터미널에서 직접 실행`
      : agent.installed && agent.profileUnsupported
        ? agent.profileUnsupported
      : connected
        ? `연결됨${version}`
        : unknown
          ? `설치됨${version} · 로그인 상태를 CLI가 알려 주지 않습니다`
          : needsLogin
            ? `설치됨${version} · 로그인이 필요합니다`
            : "설치되어 있지 않습니다";
  const Icon = connected ? Check : agent.installed ? CircleAlert : CircleDashed;
  const tone = connected ? "bg-status-success-background text-status-success" : agent.installed ? "bg-status-warning-background text-status-warning" : "bg-muted text-muted-foreground";

  let action = null;
  if (busy) action = null;
  else if (!agent.installed && agent.canInstall)
    action = (
      <button type="button" onClick={() => void state.install(agent.id)} className={BUTTON_PRIMARY}>
        <Download className="size-3.5" aria-hidden="true" />
        설치
      </button>
    );
  else if (!agent.installed)
    action = (
      <button type="button" onClick={() => state.openExternal(agent.installUrl)} disabled={!agent.installUrl} className={BUTTON_SECONDARY}>
        <ExternalLink className="size-3.5" aria-hidden="true" />
        공식 설치 페이지
      </button>
    );
  else if (agent.id === "agy")
    action = (
      <button type="button" onClick={() => void state.terminal()} className={BUTTON_SECONDARY}>
        <Terminal className="size-3.5" aria-hidden="true" />
        터미널 열기
      </button>
    );
  else if (needsLogin || unknown)
    action = (
      <button type="button" onClick={() => void state.login(agent.id)} className={needsLogin ? BUTTON_PRIMARY : BUTTON_SECONDARY}>
        <LogIn className="size-3.5" aria-hidden="true" />
        연결
      </button>
    );

  return (
    <li className="px-4 py-3.5">
      <div className="flex items-center gap-3">
        <span className={`flex size-7 shrink-0 items-center justify-center rounded-md ${tone}`}>
          <Icon className="size-4" aria-hidden="true" />
        </span>
        <div className="min-w-0 flex-1">
          <p className="text-[13px] font-medium">{agent.name}</p>
          <p className="truncate text-xs text-muted-foreground" title={agent.path}>
            {status}
          </p>
        </div>
        {action}
      </div>
      {busy && (
        <div className="relative mt-3 ml-10 h-1 overflow-hidden rounded-full bg-muted" role="progressbar" aria-label={`${agent.name} ${busy === "install" ? "설치" : busy === "terminal" ? "터미널 열기" : "로그인"} 진행 중`}>
          <span className="progress-flow absolute inset-y-0 w-2/5 rounded-full bg-primary/70" />
        </div>
      )}
      {!busy && !agent.installed && !agent.canInstall && agent.installHint && (
        <p className="mt-2 ml-10 text-xs leading-relaxed text-muted-foreground">
          Project O가 직접 설치하지 않습니다. 공식 안내대로 설치한 뒤 다시 확인을 눌러 주세요. 앱은 켜질 때의 PATH만 읽으므로, 방금 설치한 Node.js나 CLI가 그래도 보이지 않으면 앱을 다시 시작해 주세요. <span className="text-foreground/85">{agent.installHint}</span>
        </p>
      )}
      {!busy && agent.id === "agy" && agent.installed && (
        <p className="mt-2 ml-10 text-xs leading-relaxed text-muted-foreground">터미널에서 작업 폴더로 이동한 뒤 agy를 직접 입력하세요.</p>
      )}
      {!busy && agent.id !== "agy" && needsLogin && (
        <p className="mt-2 ml-10 text-xs leading-relaxed text-muted-foreground">
          연결을 누르면 공식 로그인용 콘솔 창이나 브라우저가 열립니다. 로그인을 마친 뒤 다시 확인을 눌러 주세요.
        </p>
      )}
      <div role="status" className="ml-10 empty:hidden">
        {note && (
          <div className="mt-2">
            <p className={`text-xs ${note.ok ? "text-status-success" : "text-destructive"}`}>{note.message}</p>
            {note.log && <Details label="기록 보기" text={note.log} />}
          </div>
        )}
      </div>
    </li>
  );
}

function Details({ label, text }: { label: string; text: string }) {
  return (
    <details className="mt-1.5 text-xs">
      <summary className="w-fit cursor-pointer rounded text-muted-foreground hover:text-foreground focus-ring">{label}</summary>
      <pre className="scrollbar-sleek mt-1.5 max-h-48 overflow-auto rounded-md border bg-muted/40 p-2 font-mono text-2xs leading-relaxed whitespace-pre-wrap">{text}</pre>
    </details>
  );
}
