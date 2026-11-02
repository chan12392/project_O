import { ArrowUp, FileText, Image as ImageIcon, LoaderCircle, Plus, ShieldCheck, Square, X } from "lucide-react";
import { useEffect, useRef, useState, type ClipboardEvent, type DragEvent, type FormEvent, type KeyboardEvent, type RefObject } from "react";

import {
  ATTACH_ACCEPT,
  EFFORT_WORD,
  LEAD_EFFORTS,
  LEAD_NAME,
  LEAD_PROVIDERS,
  MODEL_ID,
  attachProblem,
  effortFor,
  fileSize,
  isLeadProvider,
  modelLabel,
  type LeadProvider,
} from "../../../../src/core/lead.ts";
import { updateSettings, useSettings } from "../../data/settings.ts";
import { useLeadModels, type LeadState } from "../../data/useLead.ts";
import type { AgentStatus, Backend, LeadAttachment } from "../../platform/backend/index.ts";
import { ChoiceMenu, type MenuChoice } from "../../ui/Menu.tsx";

const MAC = typeof navigator !== "undefined" && navigator.userAgent.includes("Mac");
const SEND_KEY = MAC ? "⌘Enter" : "Ctrl+Enter";
const PLACEHOLDER = "총괄에게 맡길 일을 적어 주세요 · + 로 이미지·문서 첨부";
/** The model menu's "type an id" row; a leading space never starts a model id. */
const TYPE_MODEL = " 직접 입력";

/** One file in the composer: uploading, saved for the thread, or refused. */
interface Pending {
  key: string;
  name: string;
  size: number;
  image: boolean;
  saved?: LeadAttachment;
  error?: string;
}

const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** The agents that may lead right now: installed, signed in, and able to run read-only with resume. */
function useLeadAgents(api: Backend, clean: boolean) {
  const [agents, setAgents] = useState<AgentStatus[]>();
  useEffect(() => {
    let current = true;
    setAgents(undefined);
    api.agents(clean).then(
      (list) => current && setAgents(list),
      () => current && setAgents([]),
    );
    return () => {
      current = false;
    };
  }, [api, clean]);
  const usable = (agents ?? []).filter((a) => a.installed && a.loggedIn === true && isLeadProvider(a.id));
  /** Why a lead agent cannot be picked now; the menu shows it on the disabled row. */
  const why = (id: LeadProvider): string | undefined => {
    const status = agents?.find((a) => a.id === id);
    if (!status?.installed) return "설치되지 않았습니다 · 설정 → 에이전트";
    if (status.loggedIn === false) return "로그인이 필요합니다 · 설정 → 에이전트";
    if (status.loggedIn !== true) return "로그인 상태를 확인하지 못했습니다";
    return undefined;
  };
  return { checking: agents === undefined, usable: usable.map((a) => a.id as LeadProvider), why };
}

/**
 * The lead chat composer, after Orca's: one box with the attachments, the message, and a
 * footer — attach and run profile on the left; agent·effort, the busy mark and send (stop
 * while the lead answers) on the right. Sending goes through `useLead` (ADR-0008).
 */
export function Composer({
  inputRef,
  api,
  project,
  lead,
  onAgents,
}: {
  inputRef: RefObject<HTMLTextAreaElement | null>;
  api: Backend;
  project: string | undefined;
  lead: LeadState;
  onAgents: () => void;
}) {
  const settings = useSettings();
  const { checking, usable, why } = useLeadAgents(api, settings.clean);
  const [text, setText] = useState("");
  const [files, setFiles] = useState<Pending[]>([]);
  const [notice, setNotice] = useState<string>();
  /** `model` "" is the CLI's default (no `--model`). */
  const [picked, setPicked] = useState<{ agent: LeadProvider; model: string; effort: string }>();
  const fileInput = useRef<HTMLInputElement>(null);
  const leadButton = useRef<HTMLButtonElement>(null);

  // The message's agent: the one picked here, else Settings → 총괄, else the first that can lead.
  const agent: LeadProvider | undefined = [picked?.agent, settings.leadAgent, ...usable].find((a): a is LeadProvider => !!a && usable.includes(a));
  const mine = picked?.agent === agent ? picked : undefined;
  const effort = agent ? effortFor(agent, mine ? mine.effort : settings.leadEffort) : undefined;
  // The model: picked here for this agent, else its default in Settings → 총괄.
  const model = mine ? mine.model : agent ? (settings.leadModels[agent] ?? "") : "";
  const models = useLeadModels(api, agent);
  const [typing, setTyping] = useState<{ draft: string; error?: string }>();

  const finishTyping = (apply: boolean) => {
    if (apply && agent && effort) {
      const id = typing?.draft.trim() ?? "";
      if (!MODEL_ID.test(id)) {
        setTyping({ draft: typing?.draft ?? "", error: "영문·숫자로 시작하고 영문·숫자·._:-/[] 만 쓰는 80자 이내의 모델 id를 적어 주세요" });
        return;
      }
      setPicked({ agent, model: id, effort });
    }
    setTyping(undefined);
    leadButton.current?.focus();
  };

  const modelChoices: MenuChoice[] = agent
    ? [
        { value: "", label: "기본 모델", hint: `${LEAD_NAME[agent]}에 설정된 모델` },
        ...models.models.map((m) => ({ value: m.id, label: m.label })),
        // A typed-in or saved id the list does not have still shows, so the person sees what runs.
        ...(model && !models.models.some((m) => m.id === model) ? [{ value: model, label: model, hint: "직접 입력한 모델" }] : []),
        ...(models.loading ? [{ value: " 불러오는 중", label: "모델 목록을 불러오는 중…", disabled: true }] : []),
        ...(models.failed ? [{ value: TYPE_MODEL, label: "직접 입력…", hint: "모델 목록을 불러오지 못했습니다" }] : []),
      ]
    : [];
  const modelName = agent ? modelLabel(models.models, model) : undefined;

  const uploading = files.some((f) => !f.saved && !f.error);
  const busy = lead.busy;
  const blocker = !project
    ? "프로젝트 폴더를 먼저 정하세요"
    : !checking && !agent
      ? "총괄을 맡을 에이전트가 연결되어 있지 않습니다"
      : uploading
        ? "첨부를 올리는 중입니다"
        : undefined;
  const canSend = Boolean(text.trim()) && !blocker && !busy && Boolean(agent);

  const addFiles = (list: FileList | File[]) => {
    if (!project) return setNotice("프로젝트 폴더를 먼저 정하세요");
    let count = files.filter((f) => !f.error).length;
    const added: Pending[] = [];
    for (const file of Array.from(list)) {
      const problem = attachProblem(file.name, file.size, count);
      const item: Pending = { key: `${file.name}-${file.size}-${Date.now()}-${added.length}`, name: file.name, size: file.size, image: file.type.startsWith("image/"), error: problem };
      added.push(item);
      if (problem) continue;
      count += 1;
      api.leadAttach(project, file).then(
        (saved) => setFiles((all) => all.map((f) => (f.key === item.key ? { ...f, saved, image: saved.image } : f))),
        (e) => {
          setFiles((all) => all.map((f) => (f.key === item.key ? { ...f, error: errorText(e) } : f)));
          setNotice(`${file.name}: ${errorText(e)}`);
        },
      );
    }
    setFiles((all) => [...all, ...added]);
    // A refused file says why out loud, not only in its chip's tooltip.
    setNotice(added.find((f) => f.error)?.error);
  };

  const submit = async (event?: FormEvent) => {
    event?.preventDefault();
    if (!canSend || !agent || !project) {
      if (text.trim() && blocker) setNotice(blocker);
      return;
    }
    const message = text;
    const attachments = files.flatMap((f) => (f.saved ? [f.saved.path] : []));
    setText("");
    setFiles([]);
    setNotice(undefined);
    try {
      await lead.send({ provider: agent, model: model || undefined, effort, text: message, attachments, clean: settings.clean });
    } catch (e) {
      // Nothing was recorded: give the message back.
      setText((now) => now || message);
      setNotice(errorText(e));
    }
  };

  const stop = async () => {
    try {
      await lead.stop();
    } catch (e) {
      setNotice(errorText(e));
    }
  };

  const onKey = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Enter" && (MAC ? event.metaKey : event.ctrlKey)) {
      event.preventDefault();
      void submit();
    }
  };
  const onPaste = (event: ClipboardEvent<HTMLTextAreaElement>) => {
    if (event.clipboardData.files.length > 0) {
      event.preventDefault();
      addFiles(event.clipboardData.files);
    }
  };
  const onDrop = (event: DragEvent<HTMLFormElement>) => {
    if (event.dataTransfer.files.length === 0) return;
    event.preventDefault();
    addFiles(event.dataTransfer.files);
  };

  const quiet =
    "inline-flex h-7 items-center gap-1.5 rounded-md px-2 text-xs text-chat-foreground-faint hover:bg-accent hover:text-chat-foreground focus-ring disabled:cursor-not-allowed disabled:opacity-50";

  return (
    <form className="shrink-0 bg-chat-canvas px-6 pb-4" noValidate onSubmit={submit} onDragOver={(e) => e.dataTransfer.types.includes("Files") && e.preventDefault()} onDrop={onDrop}>
      <div className="mx-auto max-w-3xl rounded-xl border border-chat-composer-border bg-chat-composer-surface px-2.5 pt-2.5 pb-2 focus-within:ring-2 focus-within:ring-ring/50">
        {files.length > 0 && (
          <ul aria-label="첨부" className="mb-2 flex flex-wrap gap-1.5 px-0.5">
            {files.map((f) => (
              <li
                key={f.key}
                className={`flex max-w-56 items-center gap-1.5 rounded-md border py-1 pr-1 pl-2 text-xs ${f.error ? "border-destructive/50 text-destructive" : "bg-background/40 text-chat-foreground"}`}
                title={f.error ?? f.name}
              >
                {f.image ? <ImageIcon className="size-3.5 shrink-0 opacity-70" aria-hidden="true" /> : <FileText className="size-3.5 shrink-0 opacity-70" aria-hidden="true" />}
                <span className="min-w-0 truncate">{f.name}</span>
                <span className="shrink-0 text-chat-foreground-faint tabular-nums">
                  {f.error ? "첨부 안 됨" : f.saved ? fileSize(f.size) : <LoaderCircle className="size-3 animate-spin" aria-label="올리는 중" />}
                </span>
                <button
                  type="button"
                  onClick={() => setFiles((all) => all.filter((x) => x.key !== f.key))}
                  aria-label={`${f.name} 첨부 빼기`}
                  className="flex size-5 shrink-0 items-center justify-center rounded-sm hover:bg-accent focus-ring"
                >
                  <X className="size-3" aria-hidden="true" />
                </button>
              </li>
            ))}
          </ul>
        )}
        <label htmlFor="ask" className="sr-only">
          총괄에게 맡길 일
        </label>
        <textarea
          id="ask"
          ref={inputRef}
          name="ask"
          spellCheck={false}
          rows={2}
          value={text}
          placeholder={PLACEHOLDER}
          aria-describedby="ask-help"
          onChange={(e) => {
            setText(e.target.value);
            if (notice) setNotice(undefined);
          }}
          onKeyDown={onKey}
          onPaste={onPaste}
          className="block max-h-48 w-full resize-none bg-transparent px-1 text-sm text-chat-foreground outline-none placeholder:text-chat-foreground-faint"
        />
        {typing && agent && (
          <div className="mt-1.5 flex flex-wrap items-center gap-1.5 rounded-md border border-chat-composer-border bg-background/40 px-2 py-1.5">
            <label htmlFor="lead-model" className="shrink-0 text-xs text-chat-foreground">
              {LEAD_NAME[agent]} 모델 id
            </label>
            <input
              id="lead-model"
              autoFocus
              spellCheck={false}
              autoComplete="off"
              value={typing.draft}
              placeholder="예: gpt-6.1-sol"
              aria-invalid={typing.error ? true : undefined}
              aria-describedby="lead-model-help"
              onChange={(e) => setTyping({ draft: e.target.value })}
              onKeyDown={(e) => {
                // Enter applies the id here; it must not send the message.
                if (e.key === "Enter") {
                  e.preventDefault();
                  finishTyping(true);
                } else if (e.key === "Escape") {
                  e.preventDefault();
                  finishTyping(false);
                }
              }}
              className="h-7 min-w-40 flex-1 rounded-md border bg-background px-2 font-mono text-xs text-chat-foreground outline-none placeholder:text-chat-foreground-faint focus-ring aria-invalid:border-destructive"
            />
            <button type="button" onClick={() => finishTyping(true)} className="h-7 rounded-md bg-primary px-2.5 text-xs font-medium text-primary-foreground hover:bg-primary/90 focus-ring">
              적용
            </button>
            <button type="button" onClick={() => finishTyping(false)} className="h-7 rounded-md px-2 text-xs text-chat-foreground-faint hover:bg-accent hover:text-chat-foreground focus-ring">
              취소
            </button>
            <p id="lead-model-help" className={`basis-full text-2xs ${typing.error ? "text-destructive" : "text-chat-foreground-faint"}`}>
              {typing.error ?? "모델 목록을 불러오지 못해 id를 직접 받습니다. CLI가 모르는 모델이면 총괄이 오류로 답합니다."}
            </p>
          </div>
        )}
        <div className="mt-1.5 flex items-center gap-1">
          <input
            ref={fileInput}
            type="file"
            multiple
            accept={ATTACH_ACCEPT}
            className="sr-only"
            tabIndex={-1}
            aria-hidden="true"
            onChange={(e) => {
              if (e.target.files) addFiles(e.target.files);
              e.target.value = "";
            }}
          />
          <button
            type="button"
            onClick={() => fileInput.current?.click()}
            disabled={!project}
            aria-label="이미지·문서 첨부"
            title="이미지·문서 첨부 (10MB까지, 5개)"
            className="flex size-7 shrink-0 items-center justify-center rounded-full border border-chat-composer-border text-chat-foreground hover:bg-accent focus-ring disabled:cursor-not-allowed disabled:opacity-50"
          >
            <Plus className="size-4" aria-hidden="true" />
          </button>
          <ChoiceMenu
            label={`실행 프로필: ${settings.clean ? "깨끗한 에이전트" : "기본 하네스"}`}
            triggerClass={quiet}
            trigger={
              <>
                <ShieldCheck className="size-3.5 shrink-0" aria-hidden="true" />
                {settings.clean ? "깨끗한 에이전트" : "기본 하네스"}
              </>
            }
            groups={[
              {
                label: "실행 프로필 (설정 → 에이전트와 같음)",
                value: settings.clean ? "clean" : "normal",
                onChoose: (v) => updateSettings({ clean: v === "clean" }),
                choices: [
                  { value: "clean", label: "깨끗한 에이전트", hint: "개인 훅·지침·스킬·MCP 없이 공식 CLI만" },
                  { value: "normal", label: "기본 하네스", hint: "이 PC의 평소 CLI 설정 그대로" },
                ],
              },
            ]}
          />
          <div className="ml-auto flex min-w-0 items-center gap-1.5">
            {agent && effort ? (
              <ChoiceMenu
                label={`총괄 에이전트·모델·노력 수준: ${LEAD_NAME[agent]}, ${modelName ?? "기본 모델"}, ${EFFORT_WORD[effort] ?? effort}`}
                align="right"
                triggerRef={leadButton}
                stayOpen={(group, value) => group === "에이전트" || (group === "모델" && value !== TYPE_MODEL)}
                triggerClass="inline-flex h-7 min-w-0 items-center gap-1.5 rounded-md px-2 text-xs hover:bg-accent focus-ring disabled:opacity-50"
                trigger={
                  <span className="min-w-0 truncate">
                    <span className="font-medium text-chat-foreground">{LEAD_NAME[agent]}</span>
                    <span className="text-chat-foreground-faint">
                      {modelName ? ` · ${modelName}` : ""} · {EFFORT_WORD[effort] ?? effort}
                    </span>
                  </span>
                }
                groups={[
                  {
                    label: "에이전트",
                    value: agent,
                    onChoose: (v) => isLeadProvider(v) && setPicked({ agent: v, model: settings.leadModels[v] ?? "", effort: effortFor(v, effort) }),
                    // Every agent that can lead is listed; one that cannot be used now says why.
                    choices: LEAD_PROVIDERS.map((id) => ({ value: id, label: LEAD_NAME[id], disabled: !usable.includes(id), hint: usable.includes(id) ? undefined : why(id) })),
                  },
                  {
                    label: "모델",
                    value: model,
                    onChoose: (v) => (v === TYPE_MODEL ? setTyping({ draft: model }) : setPicked({ agent, model: v, effort })),
                    choices: modelChoices,
                  },
                  {
                    label: "노력 수준",
                    value: effort,
                    onChoose: (v) => setPicked({ agent, model, effort: v }),
                    choices: LEAD_EFFORTS[agent].map((e) => ({ value: e, label: EFFORT_WORD[e] ?? e })),
                  },
                ]}
              />
            ) : (
              <button type="button" onClick={onAgents} className={quiet} disabled={checking}>
                {checking ? "에이전트 확인 중…" : "총괄 에이전트 연결하기"}
              </button>
            )}
            {busy && <LoaderCircle className="size-4 shrink-0 animate-spin text-chat-foreground-faint" aria-hidden="true" />}
            {busy ? (
              <button
                type="button"
                onClick={stop}
                aria-label="총괄 중지"
                title="총괄 중지"
                className="flex size-7 shrink-0 items-center justify-center rounded-full bg-primary text-primary-foreground hover:bg-primary/90 focus-ring"
              >
                <Square className="size-3 fill-current" aria-hidden="true" />
              </button>
            ) : (
              <button
                type="submit"
                aria-label="보내기"
                title={`보내기 (${SEND_KEY})`}
                aria-disabled={!canSend}
                className="flex size-7 shrink-0 items-center justify-center rounded-full bg-primary text-primary-foreground hover:bg-primary/90 focus-ring aria-disabled:cursor-not-allowed aria-disabled:opacity-40 aria-disabled:hover:bg-primary"
              >
                <ArrowUp className="size-4" aria-hidden="true" />
              </button>
            )}
          </div>
        </div>
      </div>
      <p id="ask-help" aria-live="polite" className={notice ? "mx-auto mt-1.5 max-w-3xl px-1 text-xs text-destructive" : "sr-only"}>
        {notice ?? (busy ? "총괄이 프로젝트를 읽고 답하는 중입니다. 중지할 수 있습니다." : `${SEND_KEY}로 보내기. 총괄은 계획만 제안하고, 실행은 계획 카드의 실행 버튼으로 합니다.`)}
      </p>
    </form>
  );
}
