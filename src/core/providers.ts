import type { StreamSignal } from "./events.ts";
import type { Card, Plan, ProviderId } from "./plan.ts";

/**
 * How each official CLI is launched in headless mode and how its stream is
 * read. This is the only module that knows provider flags.
 *
 * Every launch keeps the CLI's own sign-in and permission checks: no token or
 * environment is injected, and no flag that disables approvals is ever added.
 */
export interface LaunchSpec {
  /** Executable name looked up on PATH. */
  bin: string;
  args: string[];
  /** Prompt written to stdin, when the CLI reads it from there. */
  stdin?: string;
}

export const BINARIES: Record<ProviderId, string> = { claude: "claude", codex: "codex", agy: "agy" };

export function launchSpec(card: Card, plan: Plan): LaunchSpec {
  const prompt = cardPrompt(card, plan);
  switch (card.provider) {
    case "claude":
      return {
        bin: BINARIES.claude,
        args: [
          "-p",
          "--output-format", "stream-json",
          "--verbose",
          "--permission-mode", "acceptEdits",
          ...opt("--model", card.model),
          ...opt("--effort", card.effort),
        ],
        stdin: prompt,
      };
    case "codex":
      return {
        bin: BINARIES.codex,
        args: [
          "exec",
          "--json",
          "-s", "workspace-write",
          "-C", plan.project,
          ...opt("-m", card.model),
          ...opt("-c", card.effort && `model_reasoning_effort=${card.effort}`),
          "-",
        ],
        stdin: prompt,
      };
    case "agy":
      return {
        bin: BINARIES.agy,
        args: [
          "--output-format", "stream-json",
          "--mode", "accept-edits",
          ...opt("--model", card.model),
          ...opt("--effort", card.effort),
          "-p", prompt,
        ],
      };
  }
}

/**
 * Headless runs cannot answer permission prompts, so a denied command ends
 * the turn. Tell the agents that cannot run shell commands up front.
 */
const TOOL_NOTE: Record<ProviderId, string | undefined> = {
  claude: "셸 명령은 승인할 사람이 없어 거부될 수 있습니다. 파일 읽기·쓰기 도구를 우선 쓰세요.",
  codex: undefined,
  agy: "셸 명령은 실행할 수 없습니다(헤드리스 모드라 승인 불가). 파일 읽기·쓰기 도구만 쓰세요.",
};

/** The work card text a provider receives. Kept short: it is paid for on every card. */
export function cardPrompt(card: Card, plan: Plan): string {
  const others = plan.cards.filter((c) => c.id !== card.id).map((c) => c.role);
  return [
    `[O 작업카드] ${plan.title} · ${card.role}`,
    `프로젝트: ${plan.project}`,
    ...(others.length > 0 ? [`같은 폴더에서 다른 카드(${others.join(", ")})가 동시에 일합니다. 맡은 파일 밖의 파일은 지우거나 고치지 마세요.`] : []),
    ...(TOOL_NOTE[card.provider] ? [TOOL_NOTE[card.provider] as string] : []),
    "",
    card.task.trim(),
    "",
    "끝나면 결과를 다섯 줄 이내로 요약하세요: 한 일, 바꾼 파일, 확인한 것, 남은 일.",
  ].join("\n");
}

function opt(flag: string, value: string | undefined): string[] {
  return value ? [flag, value] : [];
}

/**
 * Turns one stdout line into zero or more signals. Unknown lines are ignored
 * so a CLI update degrades to "less detail", never to a crash.
 */
export class StreamReader {
  private readonly provider: ProviderId;
  private agyText = "";

  constructor(provider: ProviderId) {
    this.provider = provider;
  }

  read(line: string): StreamSignal[] {
    let data: unknown;
    try {
      data = JSON.parse(line);
    } catch {
      return [];
    }
    if (!isRecord(data)) return [];
    switch (this.provider) {
      case "claude":
        return readClaude(data);
      case "codex":
        return readCodex(data);
      case "agy":
        return this.readAgy(data);
    }
  }

  private readAgy(data: Record<string, unknown>): StreamSignal[] {
    if (data.event === "init" && typeof data.conversation_id === "string") {
      return [{ kind: "session", sessionId: data.conversation_id }];
    }
    if (data.event === "step_update" && isRecord(data.step_update)) {
      const step = data.step_update;
      if (step.step_type === "agent_response") {
        if (typeof step.text_delta === "string") this.agyText += step.text_delta;
        if (step.state === "DONE" && this.agyText.trim()) {
          const text = this.agyText.trim();
          this.agyText = "";
          return [{ kind: "message", text }];
        }
        return [];
      }
      if (step.state === "ACTIVE" && step.step_type === "tool" && typeof step.tool_name === "string") {
        const params = isRecord(step.tool_info) ? step.tool_info.parameters : undefined;
        return [{ kind: "tool", name: step.tool_name, summary: toolSummary(params) || short(JSON.stringify(params ?? "")) }];
      }
      return [];
    }
    if (data.event === "result" && isRecord(data.result)) {
      const result = data.result;
      const text = String(result.response ?? result.error ?? "").trim();
      // agy reports SUCCESS even when a denied permission ended the turn with nothing done.
      const denied = Array.isArray(result.denied_actions)
        ? result.denied_actions.map((a) => (isRecord(a) ? String(a.display_name ?? a.action ?? "") : "")).filter(Boolean)
        : [];
      if (denied.length > 0 && !text) {
        return [{ kind: "final", ok: false, text: `권한 거부로 아무 작업 없이 끝남: ${denied.join(", ")} (헤드리스 모드는 승인할 수 없음)` }];
      }
      const ok = result.status === "SUCCESS";
      return [...limitFromText(text, ok), { kind: "final", ok, text }];
    }
    return [];
  }
}

function readClaude(data: Record<string, unknown>): StreamSignal[] {
  if (data.type === "system" && data.subtype === "init" && typeof data.session_id === "string") {
    return [{ kind: "session", sessionId: data.session_id }];
  }
  if (data.type === "assistant" && isRecord(data.message) && Array.isArray(data.message.content)) {
    const out: StreamSignal[] = [];
    for (const part of data.message.content) {
      if (!isRecord(part)) continue;
      if (part.type === "text" && typeof part.text === "string" && part.text.trim()) {
        out.push({ kind: "message", text: part.text.trim() });
      } else if (part.type === "tool_use" && typeof part.name === "string") {
        out.push({ kind: "tool", name: part.name, summary: toolSummary(part.input) });
      }
    }
    return out;
  }
  if (data.type === "rate_limit_event" && isRecord(data.rate_limit_info)) {
    const info = data.rate_limit_info;
    if (info.status !== "allowed" && typeof info.resetsAt === "number") {
      return [{ kind: "limit", until: new Date(info.resetsAt * 1000).toISOString() }];
    }
    return [];
  }
  if (data.type === "result") {
    const ok = data.is_error !== true && data.subtype === "success";
    return [{ kind: "final", ok, text: String(data.result ?? "").trim() }];
  }
  return [];
}

function readCodex(data: Record<string, unknown>): StreamSignal[] {
  if (data.type === "thread.started" && typeof data.thread_id === "string") {
    return [{ kind: "session", sessionId: data.thread_id }];
  }
  if (data.type === "item.completed" && isRecord(data.item)) {
    const item = data.item;
    if (item.type === "agent_message" && typeof item.text === "string") return [{ kind: "message", text: item.text.trim() }];
    if (item.type === "command_execution") return [{ kind: "tool", name: "shell", summary: short(item.command) }];
    if (item.type === "file_change") return [{ kind: "tool", name: "edit", summary: short(JSON.stringify(item.changes ?? "")) }];
    if (item.type === "web_search") return [{ kind: "tool", name: "web_search", summary: short(item.query) }];
    // item.type === "error" items are config warnings; the turn still runs.
    return [];
  }
  if (data.type === "turn.completed") return [{ kind: "final", ok: true, text: "" }];
  if (data.type === "turn.failed" || data.type === "error") {
    const text = String((isRecord(data.error) ? data.error.message : data.message) ?? "").trim();
    return [...limitFromText(text, false), { kind: "final", ok: false, text }];
  }
  return [];
}

const LIMIT_TEXT = /usage limit|rate limit|quota|resource[_ ]exhausted|too many requests|429/i;

function limitFromText(text: string, ok: boolean): StreamSignal[] {
  if (ok || !LIMIT_TEXT.test(text)) return [];
  return [{ kind: "limit", until: "" }];
}

function toolSummary(input: unknown): string {
  if (!isRecord(input)) return "";
  const path = input.file_path ?? input.Path ?? input.TargetFile;
  // Paths keep their last two segments; the project folder is already known.
  if (typeof path === "string") return short(path.replace(/\\/g, "/").split("/").slice(-2).join("/"));
  return short(input.command ?? input.CommandLine ?? input.pattern ?? input.url ?? input.description ?? "");
}

function short(value: unknown): string {
  const text = typeof value === "string" ? value : value === undefined ? "" : String(value);
  const line = text.replace(/\s+/g, " ").trim();
  return line.length > 120 ? `${line.slice(0, 117)}...` : line;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
