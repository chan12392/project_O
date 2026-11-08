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
  canLead,
  draftKey,
  draftOf,
  editDraft,
  effortFor,
  fileSize,
  isLeadProvider,
  modelLabel,
  restoreDraft,
  takeDraft,
  uploadDone,
  type DraftFile,
  type Drafts,
  type LeadProvider,
} from "../../../../src/core/lead.ts";
import { updateSettings, useSettings } from "../../data/settings.ts";
import { useLeadModels, type LeadState } from "../../data/useLead.ts";
import { useAgentCheck } from "../../data/agents.ts";
import type { Backend } from "../../platform/backend/index.ts";
import { useAutoGrow } from "../../lib/autoGrow.ts";
import { ChoiceMenu, type MenuChoice } from "../../ui/Menu.tsx";

const PLACEHOLDER = "총괄에게 맡길 일을 적어 주세요 · + 로 이미지·문서 첨부";
/** The run profile chip (project_O 프로필 사용, ADR-0013): short words, the tooltip says what each means. */
const PROFILE_CHIP = { clean: "project_O 프로필", own: "내 설정" } as const;
const PROFILE_TIP = {
  clean: "project_O 프로필: 카드와 총괄을 O의 깨끗한 에이전트 프로필로 실행합니다 (개인 스킬·훅·메모리 없음). 이전 작업 컨텍스트는 그대로 넣습니다.",
  own: "내 설정: 카드와 총괄을 이 PC의 평소 CLI 설정(스킬·훅·메모리)으로 실행합니다. 이전 작업 컨텍스트는 그대로 넣습니다.",
} as const;
/** The model menu's "type an id" row; a leading space never starts a model id. */
const TYPE_MODEL = " 직접 입력";

const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e));

/**
 * The agents that may lead right now (`canLead`), from the check every screen shares
 * (`data/agents.ts`). The last launch's answer counts until this one's arrives, so a lead
 * can be picked at once instead of after the 8–10 s check.
 */
function useLeadAgents(api: Backend, clean: boolean) {
  const check = useAgentCheck(api, clean);
  const agents = check.agents ?? (check.checking ? undefined : []);
  const usable = (agents ?? []).filter(canLead);
  /** Why a lead agent cannot be picked now (on the disabled row), or what to know about a usable one. */
  const why = (id: LeadProvider): string | undefined => {
    const status = agents?.find((a) => a.id === id);
    if (!status?.installed) return "설치되지 않았습니다 · 설정 → 에이전트";
    if (status.loggedIn === false) return "로그인이 필요합니다 · 설정 → 에이전트";
    if (status.loggedIn !== true) return "로그인 상태를 CLI가 알려 주지 않습니다";
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
  const [yolo, setYolo] = useState<boolean>();
  useEffect(() => {
    let live = true;
    const refresh = () => { void api.yolo().then((value) => { if (live) setYolo(value.enabled); }, () => { if (live) setYolo(undefined); }); };
    refresh();
    const timer = window.setInterval(refresh, 2000);
    return () => { live = false; window.clearInterval(timer); };
  }, [api]);
  const { checking, usable, why } = useLeadAgents(api, settings.clean);
  // Each open lead conversation keeps its own draft (ADR-0034); switching tabs shows that tab's,
  // and an upload or a failed send lands back in the draft it came from (src/core/lead.ts `Drafts`).
  const [drafts, setDrafts] = useState<Drafts>({});
  const key = project ? draftKey(project, lead.selected) : "";
  const { text, files } = draftOf(drafts, key);
  const setText = (value: string) => setDrafts((d) => editDraft(d, key, () => ({ text: value })));
  useAutoGrow(inputRef, text);
  const [notice, setNotice] = useState<string>();
  // The draft on screen now, for answers that arrive later: their notice shows only there.
  const keyNow = useRef(key);
  keyNow.current = key;
  // A notice is about the project it was raised in.
  useEffect(() => setNotice(undefined), [key]);
  // A draft typed before the answer named the project's thread moves to the key that answer
  // gives; drafts between open tabs stay where they were typed.
  const prevKey = useRef(key);
  useEffect(() => {
    const from = prevKey.current;
    prevKey.current = key;
    if (from === key || from.includes("#") || !key.includes("#")) return;
    setDrafts((d) => {
      const gone = draftOf(d, from);
      if (!gone.text && !gone.files.length) return d;
      return restoreDraft(takeDraft(d, from).drafts, key, gone);
    });
  }, [key]);
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
        // An alias (Claude's) shows itself at the row's end; the tooltip names the model it stands for now.
        ...models.models.map((m) => ({ value: m.id, label: m.label, ...(m.hint ? { aside: m.id, title: `별칭 ${m.id} → ${m.hint}` } : {}) })),
        // A typed-in or saved id the list does not have still shows, so the person sees what runs.
        ...(model && !models.models.some((m) => m.id === model) ? [{ value: model, label: model, hint: "직접 입력한 모델" }] : []),
        ...(models.loading ? [{ value: " 불러오는 중", label: "모델 목록을 불러오는 중…", disabled: true }] : []),
        ...(models.failed ? [{ value: TYPE_MODEL, label: "직접 입력…", hint: "모델 목록을 불러오지 못했습니다" }] : []),
      ]
    : [];
  const modelName = agent ? modelLabel(models.models, model) : undefined;

  const uploading = files.some((f) => !f.saved && !f.error);
  const busy = lead.busy;
  const blocker = lead.thread?.busy === "unknown"
    ? "이전 총괄 프로세스의 종료를 확인할 수 없어 새 총괄을 시작할 수 없습니다"
    : !project
    ? "프로젝트 폴더를 먼저 정하세요"
    : !checking && !agent
      ? "총괄을 맡을 에이전트가 연결되어 있지 않습니다"
      : uploading
        ? "첨부를 올리는 중입니다"
        : undefined;
  const canSend = Boolean(text.trim()) && !blocker && !busy && Boolean(agent) && yolo !== undefined;
  const dispatched = lead.thread?.events.some((event) => event.t === "lead.dispatch") ?? false;

  const addFiles = (list: FileList | File[]) => {
    if (!project) return setNotice("프로젝트 폴더를 먼저 정하세요");
    let count = files.filter((f) => !f.error).length;
    const added: DraftFile[] = [];
    // The draft these files belong to, as it is now: a later send or another project never gets them.
    const gen = draftOf(drafts, key).gen;
    for (const file of Array.from(list)) {
      const problem = attachProblem(file.name, file.size, count);
      const item: DraftFile = { key: `${file.name}-${file.size}-${Date.now()}-${added.length}`, name: file.name, size: file.size, image: file.type.startsWith("image/"), error: problem };
      added.push(item);
      if (problem) continue;
      count += 1;
      api.leadAttach(project, file, lead.selected).then(
        (saved) => setDrafts((d) => uploadDone(d, key, gen, item.key, { saved })),
        (e) => {
          setDrafts((d) => uploadDone(d, key, gen, item.key, { error: errorText(e) }));
          if (keyNow.current === key) setNotice(`${file.name}: ${errorText(e)}`);
        },
      );
    }
    setDrafts((d) => editDraft(d, key, (now) => ({ files: [...now.files, ...added] })));
    // A refused file says why out loud, not only in its chip's tooltip.
    setNotice(added.find((f) => f.error)?.error);
  };

  const submit = async (event?: FormEvent) => {
    event?.preventDefault();
    if (!canSend || !agent || !project) {
      // A send that cannot go says why, the lead still answering included.
      if (text.trim() && (blocker || busy)) setNotice(blocker ?? "총괄이 아직 답하는 중입니다. 끝나거나 중지한 뒤 보내세요");
      return;
    }
    const from = key;
    const taken = draftOf(drafts, from);
    const attachments = taken.files.flatMap((f) => (f.saved ? [f.saved.path] : []));
    setDrafts((d) => takeDraft(d, from).drafts);
    setNotice(undefined);
    try {
      await lead.send({ provider: agent, model: model || undefined, effort, text: taken.text, attachments, clean: settings.clean });
    } catch (e) {
      // Nothing was recorded: give the message and its attachments back, to the draft they came from.
      setDrafts((d) => restoreDraft(d, from, taken));
      if (keyNow.current === from) setNotice(errorText(e));
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
    if (event.key !== "Enter" || event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229) return;
    if (event.ctrlKey && !event.altKey && !event.metaKey) {
      event.preventDefault();
      const input = event.currentTarget;
      input.setRangeText("\n", input.selectionStart, input.selectionEnd, "end");
      setText(input.value);
    } else if (!event.shiftKey && !event.ctrlKey && !event.altKey && !event.metaKey) {
      event.preventDefault();
      if (!event.repeat) void submit();
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
      <div className="@container mx-auto max-w-3xl rounded-xl border border-chat-composer-border bg-chat-composer-surface px-2.5 pt-2.5 pb-2 focus-within:ring-2 focus-within:ring-ring/50">
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
                  onClick={() => setDrafts((d) => editDraft(d, key, (now) => ({ files: now.files.filter((x) => x.key !== f.key) })))}
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
          className="scrollbar-sleek block max-h-[40vh] min-h-11 w-full resize-none overflow-y-auto bg-transparent px-1 text-sm text-chat-foreground outline-none placeholder:text-chat-foreground-faint"
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
        {/* Wraps instead of overflowing in a narrow chat (1100×700 with the panel open). */}
        <div className="mt-1.5 flex flex-wrap items-center gap-1">
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
            label={`실행 프로필: ${settings.clean ? PROFILE_CHIP.clean : PROFILE_CHIP.own}`}
            title={settings.clean ? PROFILE_TIP.clean : PROFILE_TIP.own}
            triggerClass={quiet}
            trigger={
              <>
                <ShieldCheck className="size-3.5 shrink-0" aria-hidden="true" />
                {/* Narrow: the icon alone; the menu's label still names the profile. */}
                <span className="@max-md:sr-only">{settings.clean ? PROFILE_CHIP.clean : PROFILE_CHIP.own}</span>
              </>
            }
            groups={[
              {
                label: "project_O 프로필 사용 (설정 → 에이전트와 같음)",
                value: settings.clean ? "clean" : "own",
                onChoose: (v) => updateSettings({ clean: v === "clean" }),
                choices: [
                  { value: "clean", label: "project_O 프로필", hint: "개인 스킬·훅·메모리 없이 공식 CLI만 (기본)" },
                  { value: "own", label: "내 설정", hint: "이 PC의 평소 CLI 설정(스킬·훅·메모리) 그대로" },
                ],
              },
            ]}
          />
          {yolo && <span className="rounded-full border border-destructive bg-destructive px-2 py-0.5 text-xs font-semibold text-destructive-foreground" title="승인 없이 파일 수정과 명령 실행이 가능합니다">YOLO</span>}
          {yolo === undefined && <span role="status" className="text-xs text-destructive">YOLO 상태 확인 중 또는 확인 실패</span>}
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
                    choices: [...LEAD_PROVIDERS.map((id) => ({ value: id, label: LEAD_NAME[id], disabled: !usable.includes(id), hint: why(id) })), { value: "devin", label: "Devin", disabled: true, hint: "총괄은 아직 지원하지 않음" }],
                  },
                  {
                    label: "모델",
                    // A long list (Antigravity names every model at every level) scrolls; agent and effort stay in view.
                    scroll: true,
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
                title="보내기 (Enter)"
                aria-disabled={!canSend}
                className="flex size-7 shrink-0 items-center justify-center rounded-full bg-primary text-primary-foreground hover:bg-primary/90 focus-ring aria-disabled:cursor-not-allowed aria-disabled:opacity-40 aria-disabled:hover:bg-primary"
              >
                <ArrowUp className="size-4" aria-hidden="true" />
              </button>
            )}
          </div>
        </div>
      </div>
      {/* Stays mounted so a notice is announced; empty, it takes no space. */}
      <p
        id="ask-help"
        aria-live="polite"
        className={`mx-auto mt-1.5 max-w-3xl px-1 empty:mt-0 ${notice ? "text-xs text-destructive" : "truncate text-2xs text-chat-foreground-faint"}`}
      >
        {notice ?? (lead.thread?.busy === "unknown" ? blocker : busy ? "총괄이 답하는 중입니다. ■로 중지할 수 있습니다." : dispatched ? "기존 작업의 후속 요청은 자동 전달됩니다" : null)}
      </p>
    </form>
  );
}
