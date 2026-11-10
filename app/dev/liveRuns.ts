import { execFile, spawn } from "node:child_process";
import { existsSync } from "node:fs";
import type { IncomingMessage, ServerResponse } from "node:http";
import { isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import type { Plugin } from "vite";
import { MODEL_ID } from "../../src/core/lead.ts";
import { DESKTOP_UPDATES_ONLY } from "../src/platform/updates.ts";

/**
 * Prefer the `o` built for this development server, else the one on PATH.
 * The dev server reads runs and usage only through `o`, the same code the CLI
 * and the desktop shell use; it never opens O's folder itself.
 */
export function oBinary(): string {
  const root = fileURLToPath(new URL("../../", import.meta.url));
  const cargoDir = process.env.TAURI_ENV_TARGET_TRIPLE ? join(root, "app", "src-tauri") : root;
  const targetDir = process.env.CARGO_TARGET_DIR
    ? (isAbsolute(process.env.CARGO_TARGET_DIR) ? process.env.CARGO_TARGET_DIR : resolve(cargoDir, process.env.CARGO_TARGET_DIR))
    : join(root, "target");
  const target = process.env.TAURI_ENV_TARGET_TRIPLE || process.env.CARGO_BUILD_TARGET;
  const profile = process.env.TAURI_ENV_TARGET_TRIPLE && process.env.TAURI_ENV_DEBUG !== "true" ? "release" : "debug";
  const exe = `o${process.platform === "win32" ? ".exe" : ""}`;
  const candidates = [target && join(targetDir, target, profile, exe), join(targetDir, profile, exe), join(targetDir, "release", exe)];
  return candidates.find((path): path is string => !!path && existsSync(path)) ?? "o";
}

/**
 * Why `o` failed, for the page: its stderr, else the end of its stdout with the bare error
 * (an `o` too old for a command prints its usage to stdout and exits 1, which said nothing).
 */
export function failureText(stderr: string, stdout: string, message: string): string {
  if (stderr.trim()) return stderr.trim();
  const tail = stdout.trim().split(/\r?\n/).slice(-4).join("\n");
  return tail ? `${message}\n${tail}` : message;
}

/** Runs `o` with fixed arguments (no shell) and returns its stdout. */
function o(args: string[], timeout = 60_000, cwd?: string): Promise<string> {
  return new Promise((resolve, reject) =>
    execFile(oBinary(), args, { windowsHide: true, maxBuffer: 256 * 1024 * 1024, timeout, cwd }, (error, stdout, stderr) =>
      error ? reject(new Error(failureText(stderr, stdout, error.message))) : resolve(stdout),
    ),
  );
}

/** `o` with bytes on stdin (lead messages and attachments), no shell. */
function oWithInput(args: string[], input: Buffer, timeout: number): Promise<string> {
  return new Promise((resolve, reject) => {
    const child = spawn(oBinary(), args, { windowsHide: true, stdio: ["pipe", "pipe", "pipe"] });
    const out: Buffer[] = [];
    const err: Buffer[] = [];
    const timer = setTimeout(() => child.kill(), timeout);
    child.stdout.on("data", (b: Buffer) => out.push(b));
    child.stderr.on("data", (b: Buffer) => err.push(b));
    child.on("error", (e) => {
      clearTimeout(timer);
      reject(e);
    });
    child.on("close", (code) => {
      clearTimeout(timer);
      const stdout = Buffer.concat(out).toString("utf8");
      const stderr = Buffer.concat(err).toString("utf8");
      if (code === 0) resolve(stdout);
      else reject(Object.assign(new Error(failureText(stderr, stdout, `o exited with ${code}`)), { code }));
    });
    child.stdin.end(input);
  });
}

/**
 * A request body up to `limit` bytes. A larger one is refused as soon as it passes the limit,
 * and the rest is read and dropped rather than cutting the connection: cut, the page saw
 * "Failed to fetch" instead of the 413 that says why.
 */
export function body(req: IncomingMessage, limit: number): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    const parts: Buffer[] = [];
    let size = 0;
    req.on("data", (b: Buffer) => {
      size += b.length;
      if (size > limit) {
        parts.length = 0;
        reject(new Error("첨부 파일은 하나에 10MB까지입니다"));
      } else parts.push(b);
    });
    req.on("end", () => resolve(Buffer.concat(parts)));
    req.on("error", reject);
  });
}

function send(res: ServerResponse, status: number, body: string) {
  res.statusCode = status;
  res.setHeader("Content-Type", "application/json; charset=utf-8");
  res.setHeader("Cache-Control", "no-store");
  res.end(body);
}

/**
 * Only this machine's own address may ask. This middleware runs before Vite's host
 * check, so without it a site using DNS rebinding could read the run records.
 */
function localHost(req: IncomingMessage): boolean {
  return /^(localhost|127\.0\.0\.1|\[::1\])(:\d+)?$/i.test(req.headers.host ?? "");
}

/**
 * Pages on other origins (another site, or another localhost port) may neither
 * read the runs nor make this machine run the usage CLIs. Same-origin fetches
 * either send this page's own Origin or none.
 */
function sameOrigin(req: IncomingMessage): boolean {
  const origin = req.headers.origin;
  // A browser says where a request comes from even when it sends no Origin (an <img> on another site).
  const site = req.headers["sec-fetch-site"];
  if (site && site !== "same-origin" && site !== "none") return false;
  return !origin || origin === `http://${req.headers.host}`;
}

/** `/agents/<id>/<action>` with a known agent id, the only ones `o agents` takes. */
const AGENT_ACTION = /^\/agents\/(claude|codex|agy|opencode|devin)\/(install|login|setup)$/;

/** A PR number or github.com pull address, as `o review` takes it (platform/backend PR_SELECTOR). */
const PR_SELECTOR = /^(?:[1-9]\d{0,6}|https:\/\/github\.com\/[\w.-]+\/[\w.-]+\/pull\/[1-9]\d{0,6})$/;
/** An absolute folder; `o review`'s policy check decides whether it may be a project. */
const ABSOLUTE = /^(?:[A-Za-z]:[\\/]|\/(?!\/))/;
/** Settings → 에이전트 → 하루 사용 예산: the day's allowance, percent of the weekly window. */
const BUDGET_PERCENT = /^1[234]$/;

/** npm installs can take minutes. */
const INSTALL_TIMEOUT_MS = 10 * 60_000;
/** A lead turn reads the project before it answers; `o` ends it at 15 minutes (lead::TURN_LIMIT) and records why, so this waits a little longer. */
const LEAD_TIMEOUT_MS = 16 * 60_000;
/** One attachment (10MB), with room for nothing else. */
const ATTACH_LIMIT = 10 * 1024 * 1024;
/** Lead ids, efforts and plan ids as `o lead` takes them; it checks every one again. */
const LEAD_PROVIDER = /^(claude|codex|agy)$/;
const LEAD_EFFORT = /^(minimal|low|medium|high|xhigh|max)$/;
const THREAD_ID = /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/;
const PLAN_ID = /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}-p\d{1,4}$/;
const PROPOSAL_ID = /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}-f\d{1,4}$/;
/** A waiting approval's id (`<threadId>-a<n>`, crates/o-core/src/lead/approvals.rs); `o lead decide` checks it again. */
const APPROVAL_ID = /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}-a\d{1,6}$/;
/** Terminal session ids as `o term` makes them (crates/o-core/src/term `is_session_id`); it checks them again. */
const TERM_ID = /^t-\d{8}-\d{6}-[A-Za-z0-9]{4}$/;
/** How long a lead model list is reused before the CLI is asked again (the Tauri `lead_models` keeps it as long). */
const MODELS_TTL_MS = 10 * 60_000;
const leadModels = new Map<string, { at: number; out: string }>();

/** `o lead models`, kept a few minutes per agent; a failed listing is not kept. */
function cachedModels(provider: string): Promise<string> {
  const hit = leadModels.get(provider);
  if (hit && Date.now() - hit.at < MODELS_TTL_MS) return Promise.resolve(hit.out);
  return o(["lead", "models", "--provider", provider]).then((out) => {
    leadModels.set(provider, { at: Date.now(), out });
    return out;
  });
}

/**
 * `o lead stop` for one project; none without an existing absolute folder. Without
 * `--project`, `o lead stop` stops every project's lead, which a page never asks for.
 */
export function leadStopArgs(project: string): string[] | undefined {
  return ABSOLUTE.test(project) && existsSync(project) ? ["lead", "stop", "--project", project] : undefined;
}

/** The lead chat's twins of the Tauri lead_* commands (app/src-tauri/src/commands/lead.rs). */
async function lead(req: IncomingMessage, res: ServerResponse, path: string, query: URLSearchParams): Promise<boolean> {
  const fail = (status: number, message: string) => send(res, status, JSON.stringify({ error: message }));
  const run = (p: Promise<string>) =>
    void p.then(
      (out) => send(res, 200, out),
      // Exit code 2 is a policy refusal: its reasons are the answer.
      (e: Error & { code?: number }) => fail(e.code === 2 ? 409 : 500, e.message),
    );
  const project = query.get("project") ?? "";
  const projectOk = ABSOLUTE.test(project) && existsSync(project);
  if (path === "/lead/schedule" && req.method === "GET") { run(o(["schedule", "list"])); return true; }
  if (path === "/lead/schedule" && req.method === "POST") {
    try { run(oWithInput(["schedule", "create", "-"], await body(req, 64 * 1024), 60_000)); }
    catch (e) { fail(413, (e as Error).message); }
    return true;
  }
  if (path === "/lead/schedule/delete" && req.method === "POST") { run(o(["schedule", "delete", query.get("id") ?? ""])); return true; }
  if ((path === "/lead/schedule/enable" || path === "/lead/schedule/disable") && req.method === "POST") { run(o(["schedule", path.endsWith("/enable") ? "enable" : "disable", query.get("id") ?? ""])); return true; }
  if (path.startsWith("/lead/image/")) {
    const action = path.slice("/lead/image/".length);
    const allowed = (action === "read" && req.method === "GET") || ((action === "open" || action === "folder") && req.method === "POST");
    if (!allowed || !projectOk) { fail(400, "이미지 요청이 올바르지 않습니다"); return true; }
    run(o(["image", action, project, query.get("run") || "-", query.get("card") || "-", query.get("path") || ""]));
    return true;
  }
  if (req.method === "GET" && path === "/lead/show") {
    const thread = query.get("thread");
    if ((query.has("project") && !projectOk) || (thread !== null && !THREAD_ID.test(thread))) {
      fail(400, "프로젝트 폴더 또는 대화 id가 올바르지 않습니다");
      return true;
    }
    run(o(["lead", "show", "--json", ...(projectOk ? ["--project", project] : []), ...(thread === null ? [] : ["--thread", thread])]));
    return true;
  }
  if (req.method === "GET" && path === "/lead/models") {
    const provider = query.get("provider") ?? "";
    if (!LEAD_PROVIDER.test(provider)) {
      fail(400, "에이전트가 필요합니다");
      return true;
    }
    run(cachedModels(provider));
    return true;
  }
  if (req.method !== "POST" || !path.startsWith("/lead/")) return false;
  if (path === "/lead/close" || path === "/lead/writer") {
    const thread = query.get("thread") ?? "";
    if (!THREAD_ID.test(thread)) { fail(400, "대화 id 형식이 아닙니다"); return true; }
    run(o(["lead", path === "/lead/close" ? "close" : "writer", thread]));
    return true;
  }
  if (path === "/lead/wake") {
    const runId = query.get("run") ?? "";
    if (!/^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(runId)) { fail(400, "실행 ID가 올바르지 않습니다"); return true; }
    run(oWithInput(["lead", "wake", runId], Buffer.alloc(0), LEAD_TIMEOUT_MS));
    return true;
  }
  if (path === "/lead/retry") {
    const runId = query.get("run") ?? "";
    const messageId = query.get("message") ?? "";
    if (!/^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(runId) || !/^[A-Za-z0-9_-]{1,160}$/.test(messageId)) { fail(400, "실행 또는 메시지 ID가 올바르지 않습니다"); return true; }
    run(o(["lead", "retry", runId, messageId]));
    return true;
  }
  if (path === "/lead/answer-approval") {
    const runId = query.get("run") ?? "";
    const messageId = query.get("message") ?? "";
    if (!/^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(runId) || !/^[A-Za-z0-9_-]{1,160}$/.test(messageId)) { fail(400, "실행 또는 메시지 ID가 올바르지 않습니다"); return true; }
    try { run(oWithInput(["msg", "answer-approval", "--run", runId, "--id", messageId, "-"], await body(req, 65_536), 60_000)); }
    catch (e) { fail(413, (e as Error).message); }
    return true;
  }
  if (path === "/lead/followup") {
    const runId = query.get("run") ?? "";
    const card = query.get("card") ?? "";
    if (!/^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(runId) || !/^[a-z0-9][a-z0-9-]{0,63}$/.test(card)) { fail(400, "실행 또는 작업 ID가 올바르지 않습니다"); return true; }
    try { run(oWithInput(["followup", runId, card], await body(req, 64 * 1024), 60_000)); }
    catch (e) { fail(413, (e as Error).message); }
    return true;
  }
  // The Tauri run_reply twin: the person's answer to a card's question, sent as the lead.
  if (path === "/lead/reply") {
    const runId = query.get("run") ?? "";
    const card = query.get("card") ?? "";
    const message = query.get("message");
    if (!/^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(runId) || !/^[a-z0-9][a-z0-9-]{0,63}$/.test(card)) { fail(400, "실행 또는 작업 ID가 올바르지 않습니다"); return true; }
    if (message !== null && !/^[A-Za-z0-9_-]{1,160}$/.test(message)) { fail(400, "메시지 ID가 올바르지 않습니다"); return true; }
    let text: string;
    try { text = (await body(req, 64 * 1024)).toString("utf8").trim(); }
    catch (e) { fail(413, (e as Error).message); return true; }
    if (!text) { fail(400, "보낼 답이 비어 있습니다"); return true; }
    // `o msg` reads a word starting with "--" as a flag; a leading space keeps the answer a word.
    run(o(["msg", "send", "--run", runId, "--as", "lead", "--to", card, ...(message ? ["--reply-to", message] : []), text.startsWith("-") ? ` ${text}` : text]).then(() => "{}"));
    return true;
  }
  if (path === "/lead/note") {
    const thread = query.get("thread") ?? "";
    if (!/^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(thread)) { fail(400, "대화 id 형식이 아닙니다"); return true; }
    try { run(oWithInput(["lead", "note", thread, "-"], await body(req, 64 * 1024), 60_000)); }
    catch (e) { fail(413, (e as Error).message); }
    return true;
  }
  // The Tauri lead_decide twin: the person's 허용/거부 for one action the lead waits on.
  if (path === "/lead/decide") {
    const id = query.get("id") ?? "";
    const decision = query.get("decision") ?? "";
    if (!APPROVAL_ID.test(id) || (decision !== "allow" && decision !== "deny")) { fail(400, "승인 요청 id나 결정이 올바르지 않습니다"); return true; }
    run(o(["lead", "decide", id, decision]));
    return true;
  }
  if (path === "/lead/stop") {
    const thread = query.get("thread");
    if (thread !== null && !THREAD_ID.test(thread)) { fail(400, "대화 id 형식이 아닙니다"); return true; }
    const args = thread !== null && !query.has("project") ? ["lead", "stop"] : leadStopArgs(project);
    if (!args) {
      fail(400, "중지할 총괄의 프로젝트 폴더가 필요합니다");
      return true;
    }
    run(o([...args, ...(thread === null ? [] : ["--thread", thread])]));
    return true;
  }
  if (path === "/lead/stop-all") {
    if (!projectOk) { fail(400, "프로젝트 폴더가 필요합니다"); return true; }
    run(o(["lead", "stop-all", "--project", project]));
    return true;
  }
  if (path === "/lead/dispatch") {
    const plan = query.get("plan") ?? "";
    if (!PLAN_ID.test(plan)) {
    fail(400, "계획 id 형식이 아닙니다");
    return true;
  }
    run(o(["about", "--json"]).then((text) => {
      const caps: unknown = JSON.parse(text).capabilities;
      if (!Array.isArray(caps) || !caps.includes("work-graph-v1") || !caps.includes("lead-worktrees-v1") || !caps.includes("lead-copy-workspaces-v1")) {
        throw new Error("O CLI가 작업 그래프와 워크트리 격리를 지원하지 않습니다. 앱과 같은 버전의 O CLI를 다시 빌드하세요");
      }
      return oWithInput(["lead", "dispatch", plan, "--profile", query.get("clean") === "1" ? "project-o" : "own"], Buffer.alloc(0), 60_000);
    }));
    return true;
  }
  if (path === "/lead/forward") {
    const proposal = query.get("proposal") ?? "";
    if (!PROPOSAL_ID.test(proposal)) { fail(400, "요청 제안 id 형식이 아닙니다"); return true; }
    run(o(["about", "--json"]).then((text) => {
      const caps: unknown = JSON.parse(text).capabilities;
      if (!Array.isArray(caps) || !caps.includes("overall-review-v1")) throw new Error("이 O CLI는 전체 총괄의 요청 전달을 지원하지 않습니다. 앱과 같은 버전의 O CLI를 다시 빌드하세요");
      return o(["lead", "forward", proposal]);
    }));
    return true;
  }
  if (!projectOk) {
    fail(400, "프로젝트 폴더가 필요합니다");
    return true;
  }
  if (path === "/lead/new") {
    run(o(["lead", "new", "--project", project]));
    return true;
  }
  if (path === "/lead/attach") {
    const thread = query.get("thread");
    if (thread !== null && !THREAD_ID.test(thread)) { fail(400, "대화 id 형식이 아닙니다"); return true; }
    const name = query.get("name") ?? "";
    if (!name) {
    fail(400, "파일 이름이 필요합니다");
    return true;
  }
    try {
      const bytes = await body(req, ATTACH_LIMIT);
      run(oWithInput(["lead", "attach", "--project", project, ...(thread === null ? [] : ["--thread", thread]), "--name", name, "-"], bytes, 60_000));
    } catch (e) {
      fail(413, (e as Error).message);
    }
    return true;
  }
  if (path === "/lead/send") {
    let input: { thread?: unknown; provider?: unknown; model?: unknown; effort?: unknown; text?: unknown; attachments?: unknown; clean?: unknown };
    try {
      input = JSON.parse((await body(req, 256 * 1024)).toString("utf8"));
    } catch {
      {
    fail(400, "요청 본문이 JSON이 아닙니다");
    return true;
  }
    }
    if (!input || typeof input !== "object" || Array.isArray(input)) {
      fail(400, "요청 본문이 객체가 아닙니다");
      return true;
    }
    if (input.thread !== undefined && (typeof input.thread !== "string" || !THREAD_ID.test(input.thread))) {
      fail(400, "대화 id 형식이 아닙니다");
      return true;
    }
    const provider = typeof input.provider === "string" && LEAD_PROVIDER.test(input.provider) ? input.provider : "";
    const effort = typeof input.effort === "string" && LEAD_EFFORT.test(input.effort) ? ["--effort", input.effort] : [];
    const files = Array.isArray(input.attachments) ? input.attachments.filter((a): a is string => typeof a === "string" && ABSOLUTE.test(a)) : [];
    if (!provider || typeof input.text !== "string" || !input.text.trim()) {
    fail(400, "에이전트와 보낼 내용이 필요합니다");
    return true;
  }
    // A model that does not look like one is refused, not dropped: the lead must not quietly run another model.
    if (input.model !== undefined && input.model !== "" && (typeof input.model !== "string" || !MODEL_ID.test(input.model))) {
      fail(400, "model 이름 형식이 올바르지 않습니다");
      return true;
    }
    const model = typeof input.model === "string" && input.model ? ["--model", input.model] : [];
    const args = ["lead", "send", "--manual-dispatch", "--project", project, ...(typeof input.thread === "string" ? ["--thread", input.thread] : []), "--provider", provider, ...model, ...effort, ...files.flatMap((f) => ["--attach", f])];
    run(oWithInput([...args, "--profile", input.clean === true ? "project-o" : "own", "-"], Buffer.from(input.text, "utf8"), LEAD_TIMEOUT_MS));
    return true;
  }
  return false;
}

const GRAPH_PROVIDER = /^(claude|codex)$/;
const GRAPH_TIMEOUT_MS = 11 * 60_000;
/** The Graph settings `o graph settings` takes, as flags (ADR-0033); anything else is refused. */
const GRAPH_SWITCHES = { effects: "--effects", particles: "--particles", warnDeep: "--warn-deep" } as const;
const GRAPH_VALUES = { deepProvider: "--deep-provider", codexModel: "--codex-model", claudeModel: "--claude-model", maxNodes: "--max-nodes", depth: "--depth", colorBy: "--color-by" } as const;
/** An empty model is the CLI's own default; `o graph settings` takes it as the word `default`. */
const GRAPH_EMPTY_MODEL = new Set(["codexModel", "claudeModel"]);

/** The Graph tab's twins of the Tauri graph_* commands (app/src-tauri/src/commands/graph.rs, ADR-0033). */
async function graph(req: IncomingMessage, res: ServerResponse, path: string, query: URLSearchParams): Promise<boolean> {
  const fail = (status: number, message: string) => {
    send(res, status, JSON.stringify({ error: message }));
    return true;
  };
  const run = (args: string[], timeout?: number) => {
    void o(["graph", ...args, "--json"], timeout).then((out) => send(res, 200, out), (e: Error & { code?: number }) => fail(e.code === 2 ? 409 : 500, e.message));
    return true;
  };
  const project = query.get("project") ?? "";
  if (project && !(ABSOLUTE.test(project) && existsSync(project))) return fail(400, "프로젝트 폴더가 없습니다");
  if (req.method === "GET" && path === "/graph/view") return run(["view", ...(project ? ["--project", project] : [])]);
  if (req.method === "GET" && path === "/graph/part") return project ? run(["part", "--project", project]) : fail(400, "프로젝트가 필요합니다");
  if (req.method === "GET" && path === "/graph/settings") return run(["settings"]);
  if (req.method !== "POST") return false;
  let input: Record<string, unknown> = {};
  if (path === "/graph/deep" || path === "/graph/settings") {
    try {
      const parsed: unknown = JSON.parse((await body(req, 4096)).toString("utf8") || "{}");
      if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return fail(400, "요청 본문이 JSON 객체가 아닙니다");
      input = parsed as Record<string, unknown>;
    } catch {
      return fail(400, "요청 본문이 JSON이 아닙니다");
    }
  }
  if (path === "/graph/build") return project ? run(["build", "--project", project], GRAPH_TIMEOUT_MS) : fail(400, "프로젝트가 필요합니다");
  if (path === "/graph/deep") {
    if (!project) return fail(400, "프로젝트가 필요합니다");
    const args = ["deep", "--project", project, "--profile", input.clean === true ? "project-o" : "own"];
    if (input.provider != null) {
      if (typeof input.provider !== "string" || !GRAPH_PROVIDER.test(input.provider)) return fail(400, "Deep 에이전트는 codex 또는 claude입니다");
      args.push("--provider", input.provider);
    }
    if (input.model != null && input.model !== "") {
      if (typeof input.model !== "string" || !MODEL_ID.test(input.model)) return fail(400, "model 이름 형식이 올바르지 않습니다");
      args.push("--model", input.model);
    }
    return run(args, GRAPH_TIMEOUT_MS);
  }
  if (path === "/graph/settings") {
    const args = ["settings"];
    for (const [key, value] of Object.entries(input)) {
      if (GRAPH_EMPTY_MODEL.has(key) && value === "") args.push(GRAPH_VALUES[key as keyof typeof GRAPH_VALUES], "default");
      else if (key in GRAPH_SWITCHES && typeof value === "boolean") args.push(GRAPH_SWITCHES[key as keyof typeof GRAPH_SWITCHES], value ? "on" : "off");
      else if (key in GRAPH_VALUES && (typeof value === "string" || Number.isInteger(value)) && /^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/.test(String(value))) args.push(GRAPH_VALUES[key as keyof typeof GRAPH_VALUES], String(value));
      else return fail(400, `설정이 올바르지 않습니다: ${key}`);
    }
    return run(args);
  }
  return false;
}

/** A document path as `o dream read` takes it: relative, Markdown, no climbing out, never read as an option; it checks the list again. */
const DREAM_DOC = /^(?![\\/-])(?!.*(?:^|[\\/])\.\.(?:[\\/]|$))[^:*?"<>|\0]{1,400}\.md$/i;

/** The Wiki and Dreaming tabs' twins of the Tauri dream_* commands (app/src-tauri/src/commands/dream.rs, ADR-0041). */
function dream(req: IncomingMessage, res: ServerResponse, path: string, query: URLSearchParams): boolean {
  const fail = (status: number, message: string) => {
    send(res, status, JSON.stringify({ error: message }));
    return true;
  };
  const run = (work: Promise<string>) => {
    void work.then((out) => send(res, 200, out), (e: Error) => fail(500, e.message));
    return true;
  };
  if (req.method === "GET" && path === "/dream/changes") return run(o(["dream", "changes", "--json"]));
  if (req.method === "POST" && path === "/dream/revert") {
    return run(
      body(req, 4096).then((input) => {
        const { id, reverted } = JSON.parse(input.toString("utf8")) as { id?: unknown; reverted?: unknown };
        if (typeof id !== "string" || !/^[0-9a-f]{16}$/.test(id)) throw new Error("변경 ID가 올바르지 않습니다");
        return o(["dream", reverted === false ? "restore" : "revert", id]);
      }),
    );
  }
  if (req.method === "GET" && (path === "/dream/list" || path === "/dream/graph")) return run(o(["dream", path.slice("/dream/".length), "--json"]));
  if (req.method === "GET" && path === "/dream/read") {
    const document = query.get("path") ?? "";
    return DREAM_DOC.test(document) ? run(o(["dream", "read", document, "--json"])) : fail(400, "문서 경로가 올바르지 않습니다");
  }
  if (req.method === "POST" && path === "/dream/add") {
    // The material goes to `o dream add -` as sent: it checks every field itself.
    return run(body(req, 8 * 1024 * 1024).then((input) => oWithInput(["dream", "add", "-"], input, 60_000), () => Promise.reject(new Error("자료가 너무 큽니다"))));
  }
  if (req.method === "POST" && path === "/dream/remove") {
    return run(
      body(req, 4096).then((input) => {
        const document = (JSON.parse(input.toString("utf8")) as { path?: unknown }).path;
        if (typeof document !== "string" || !DREAM_DOC.test(document)) throw new Error("문서 경로가 올바르지 않습니다");
        return o(["dream", "remove", document]);
      }),
    );
  }
  return false;
}

/** A new project's folder name as `o projects new` takes it; it checks the name again. */
const PROJECT_NAME = /^[^\\/:*?"<>|\0]{1,64}$/;

/** The project list's twins of the Tauri projects_* commands (app/src-tauri/src/commands/projects.rs, ADR-0009). */
async function projects(req: IncomingMessage, res: ServerResponse, path: string, query: URLSearchParams): Promise<boolean> {
  // `o projects` refuses with the reason on stderr; the screen shows it as it is.
  const run = (args: string[]) => {
    void o(["projects", ...args, "--json"]).then((out) => send(res, 200, out), (e: Error) => send(res, 400, JSON.stringify({ error: e.message })));
    return true;
  };
  const fail = (message: string) => {
    send(res, 400, JSON.stringify({ error: message }));
    return true;
  };
  const folder = query.get("path") ?? "";
  if (req.method === "GET" && path === "/projects") return run([]);
  if (req.method !== "POST") return false;
  if (path === "/projects/open") {
    if (!ABSOLUTE.test(folder)) return fail("절대 경로의 폴더가 필요합니다");
    return run(["open", folder]);
  }
  if (path === "/projects/add" || path === "/projects/remove") {
    if (!ABSOLUTE.test(folder)) return fail("절대 경로의 폴더가 필요합니다");
    return run([path.endsWith("add") ? "add" : "remove", folder]);
  }
  if (path === "/projects/new") {
    const name = query.get("name") ?? "";
    if (!PROJECT_NAME.test(name)) return fail("프로젝트 이름은 1~64자이고 \\ / : * ? \" < > |를 쓸 수 없습니다");
    return run(["new", name]);
  }
  if (path === "/projects/root") {
    if (folder && !ABSOLUTE.test(folder)) return fail("절대 경로의 폴더가 필요합니다");
    const target = query.get("reset") === "1" ? ["--default"] : folder ? [folder] : [];
    return run(["root", ...target, ...(query.get("create") === "1" ? ["--create"] : [])]);
  }
  if (path === "/projects/check") {
    let paths: unknown;
    try {
      paths = JSON.parse((await body(req, 256 * 1024)).toString("utf8"));
    } catch {
      return fail("요청 본문이 JSON이 아닙니다");
    }
    const folders = Array.isArray(paths) ? paths.filter((p): p is string => typeof p === "string" && ABSOLUTE.test(p)).slice(0, 200) : [];
    if (folders.length === 0) {
      send(res, 200, "[]");
      return true;
    }
    return run(["check", ...folders]);
  }
  return false;
}

/**
 * The terminal sessions' twins of the Tauri term_* commands (app/src-tauri/src/commands/term.rs, ADR-0042).
 * Only the read routes and resize are offered: a local process can forge Origin, so
 * the dev server cannot tell a person from an agent (ADR-0042 구현 메모) — the
 * human-only operations (open, write, paste, close, `agy` changes) live in the
 * Tauri commands and `o` itself instead.
 */
export async function term(req: IncomingMessage, res: ServerResponse, path: string, query: URLSearchParams): Promise<boolean> {
  const fail = (status: number, message: string) => {
    send(res, status, JSON.stringify({ error: message }));
    return true;
  };
  const run = (p: Promise<string>) => {
    void p.then((out) => send(res, 200, out || "{}"), (e: Error & { code?: number }) => fail(e.code === 2 ? 409 : 500, e.message));
    return true;
  };
  const jsonBody = async () => {
    try {
      const data: unknown = JSON.parse((await body(req, 65_536)).toString("utf8"));
      return data && typeof data === "object" ? (data as Record<string, unknown>) : undefined;
    } catch {
      return undefined;
    }
  };
  const idOf = (data: Record<string, unknown> | undefined): string | undefined => (typeof data?.id === "string" && TERM_ID.test(data.id) ? data.id : undefined);
  if (req.method === "GET" && path === "/term") return run(o(["term", "list", "--json"]));
  if (req.method === "GET" && path === "/term/read") {
    const id = query.get("id") ?? "";
    const offset = query.get("offset") ?? "";
    if (!TERM_ID.test(id) || !/^\d{1,20}$/.test(offset)) return fail(400, "터미널 ID나 위치가 올바르지 않습니다");
    return run(o(["term", "read", id, "--offset", offset, "--json"]));
  }
  if (req.method !== "POST") return false;
  // Human-only mutations are deliberately absent (see the comment above `term`).
  if (path === "/term/resize") {
    const input = await jsonBody();
    const id = idOf(input);
    const dim = (v: unknown): v is number => Number.isInteger(v) && (v as number) >= 1 && (v as number) <= 32767;
    if (!id || !dim(input?.cols) || !dim(input?.rows)) return fail(400, "터미널 크기가 올바르지 않습니다");
    return run(o(["term", "resize", id, String(input.cols), String(input.rows)]));
  }
  return false;
}

/**
 * Dev-server stand-in for the desktop shell (src/platform/backend, src/data/useLiveRuns.ts): runs, usage,
 * agent CLI status, the official install and sign-in, and About, each through `o`.
 */
export function liveRuns(): Plugin {
  let refreshing: Promise<string> | undefined;
  const error = (res: ServerResponse) => (e: Error) => send(res, 500, JSON.stringify({ error: e.message }));
  return {
    name: "project-o-live-runs",
    apply: "serve",
    configureServer(server) {
      const monitor = spawn(oBinary(), ["_lead-watch"], { windowsHide: true, stdio: ["pipe", "ignore", "ignore"] });
      let closing = false;
      monitor.stdin.on("error", () => {});
      monitor.on("error", (error) => server.config.logger.warn(`총괄 대기 메시지 감시를 시작하지 못했습니다: ${error.message}`));
      monitor.on("exit", (code) => { if (!closing) server.config.logger.warn(`총괄 대기 메시지 감시가 종료됐습니다 (${code ?? "signal"})`); });
      server.httpServer?.once("close", () => { closing = true; monitor.kill(); });
      server.middlewares.use("/api", (req, res, next) => {
        const path = req.url?.split("?")[0];
        if (!localHost(req)) return send(res, 403, JSON.stringify({ error: "이 PC의 주소로만 열 수 있습니다" }));
        if (!sameOrigin(req)) return send(res, 403, JSON.stringify({ error: "다른 출처의 요청은 받지 않습니다" }));
        // ?sessions=1: this PC's recent Claude Code sessions too (`o runs --json --sessions`, read-only).
        const withSessions = new URLSearchParams(req.url?.split("?")[1] ?? "").get("sessions") === "1" ? ["--sessions"] : [];
        if (req.method === "GET" && path === "/runs") return void o(["runs", "--json", ...withSessions]).then((out) => send(res, 200, out), error(res));
        if (req.method === "GET" && path === "/resources") return void o(["resources", "--json"]).then((out) => send(res, 200, out), error(res));
        if (req.method === "POST" && path === "/resources/recover") {
          const params = new URLSearchParams(req.url?.split("?")[1] ?? "");
          const id = params.get("id") ?? "";
          if (!/^[1-9][0-9]{0,18}$/.test(id)) return send(res, 400, JSON.stringify({ error: "예약 ID가 올바르지 않습니다" }));
          const confirmed = params.get("confirmExited");
          if (confirmed !== null && confirmed !== "1") return send(res, 400, JSON.stringify({ error: "종료 확인 값이 올바르지 않습니다" }));
          return void o(["resources", "recover", id, ...(confirmed === "1" ? ["--confirm-exited"] : [])]).then(() => send(res, 200, JSON.stringify({ ok: true })), error(res));
        }
        if (req.method === "GET" && path === "/usage") return void o(["usage", "--cached", "--json"]).then((out) => send(res, 200, out), error(res));
        if (req.method === "POST" && path === "/usage/refresh") {
          // Whose login to ask (`o usage --profile`); anything else leaves the choice to `o`.
          const profile = new URLSearchParams(req.url?.split("?")[1] ?? "").get("profile") ?? "";
          refreshing ??= o(["usage", "--json", ...(profile === "project-o" || profile === "own" ? ["--profile", profile] : [])]).finally(() => (refreshing = undefined));
          return void refreshing.then((out) => send(res, 200, out), error(res));
        }
        const query = new URLSearchParams(req.url?.split("?")[1] ?? "");
        if (req.method === "GET" && path === "/changes") {
          const run = query.get("run") ?? "";
          const card = query.get("card") ?? "";
          if (!/^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(run) || !/^[a-z0-9][a-z0-9-]{0,63}$/.test(card)) return send(res, 400, JSON.stringify({ error: "실행 또는 작업 ID가 올바르지 않습니다" }));
          return void o(["changes", run, card]).then((out) => send(res, 200, out), error(res));
        }
        if (req.method === "GET" && path === "/budget") return void o(["budget", "--json", "--cached"]).then((out) => send(res, 200, out), error(res));
        if (req.method === "POST" && path === "/budget/percent") {
          // `o budget` checks the range again; only these three ever reach it.
          const percent = query.get("value") ?? "";
          if (!BUDGET_PERCENT.test(percent)) return send(res, 400, JSON.stringify({ error: "하루 사용 예산은 12, 13, 14 중 하나입니다" }));
          return void o(["budget", "--json", "--cached", "--percent", percent]).then((out) => send(res, 200, out), error(res));
        }
        if (req.method === "POST" && path === "/budget/enabled") {
          // 하루 사용 예산 켜기·끄기 (ADR-0013): `o budget --on|--off`.
          const value = query.get("value");
          if (value !== "on" && value !== "off") return send(res, 400, JSON.stringify({ error: "하루 사용 예산은 on 또는 off입니다" }));
          return void o(["budget", "--json", "--cached", `--${value}`]).then((out) => send(res, 200, out), error(res));
        }
        if (req.method === "GET" && path === "/ram") return void o(["ram", "--json"]).then((out) => send(res, 200, out), error(res));
        if (req.method === "POST" && path === "/ram") {
          // RAM 상한 (ADR-0037): `o ram --on|--off --percent <n>`; `o ram` checks the range again.
          const args = ["ram", "--json"];
          const enabled = query.get("enabled");
          const percent = query.get("percent");
          if (enabled !== null && enabled !== "on" && enabled !== "off") return send(res, 400, JSON.stringify({ error: "RAM 상한은 on 또는 off입니다" }));
          if (percent !== null && !/^(5\d|[6-8]\d|9[0-5])$/.test(percent)) return send(res, 400, JSON.stringify({ error: "RAM 상한은 50~95 사이의 정수입니다" }));
          if (enabled !== null) args.push(`--${enabled}`);
          if (percent !== null) args.push("--percent", percent);
          return void o(args).then((out) => send(res, 200, out), error(res));
        }
        if (req.method === "GET" && path === "/billing") return void o(["billing", "--json"]).then((out) => send(res, 200, out), error(res));
        if (req.method === "POST" && path === "/billing") {
          // API 과금 (ADR-0035 개정 1): `o billing --warn|--subscription`.
          const mode = query.get("mode");
          if (mode !== "warn" && mode !== "subscription") return send(res, 400, JSON.stringify({ error: "API 과금 설정은 warn 또는 subscription입니다" }));
          return void o(["billing", "--json", `--${mode}`]).then((out) => send(res, 200, out), error(res));
        }
          if (req.method === "GET" && path === "/yolo") return void o(["yolo", "--json"]).then((out) => send(res, 200, out), error(res));
          if (req.method === "POST" && path === "/yolo") {
            void (async () => {
            let change: { enabled?: boolean; maxCards?: number; perHour?: number; dedupeMinutes?: number };
            try { change = JSON.parse((await body(req, 4096)).toString("utf8")); }
            catch { return send(res, 400, JSON.stringify({ error: "설정이 올바르지 않습니다" })); }
            if (!change || typeof change !== "object" || (change.enabled !== undefined && typeof change.enabled !== "boolean") ||
                [change.maxCards, change.perHour, change.dedupeMinutes].some((v) => v !== undefined && (!Number.isInteger(v) || typeof v !== "number"))) {
              return send(res, 400, JSON.stringify({ error: "설정이 올바르지 않습니다" }));
            }
            const args = ["yolo", "--json"];
            if (change.maxCards !== undefined) args.push("--max-cards", String(change.maxCards));
            if (change.perHour !== undefined) args.push("--per-hour", String(change.perHour));
            if (change.dedupeMinutes !== undefined) args.push("--dedupe-minutes", String(change.dedupeMinutes));
            if (change.enabled !== undefined) args.push(change.enabled ? "--on" : "--off");
            return void o(args).then((out) => send(res, 200, out), error(res));
            })();
            return;
          }
        // Antigravity state is read-only here: a local process can forge Origin, so the
        // dev server cannot tell a person from an agent (ADR-0042 구현 메모). `o agy`
        // changes run through the Tauri command or `o` itself.
        if (req.method === "GET" && path === "/agy") return void o(["agy", "--json"]).then((out) => send(res, 200, out), error(res));
        if (path === "/term" || path?.startsWith("/term/")) {
          return void term(req, res, path, query).then((handled) => (handled ? undefined : next()), error(res));
        }
        const clean = query.get("clean") === "1" ? ["--clean"] : [];
        if (req.method === "GET" && path === "/agents") return void o(["agents", "status", "--json", ...clean]).then((out) => send(res, 200, out), error(res));
        if (req.method === "GET" && path === "/agents/catalog") return void o(["agents", "list", "--all", "--json"]).then((out) => send(res, 200, out), error(res));
        if (req.method === "POST" && path === "/agents/terminal") return void o(["agents", "terminal", "--json"]).then((out) => send(res, 200, out), error(res));
        if (req.method === "GET" && path === "/about") return void o(["about", "--json"]).then((out) => send(res, 200, JSON.stringify({ ...JSON.parse(out), commit: "", builtAt: "" })), error(res));
        if (req.method === "GET" && path === "/updates") return send(res, 200, JSON.stringify({ release: null, message: DESKTOP_UPDATES_ONLY }));
        if (req.method === "GET" && path === "/issues/search") return void o(["issues", "search", query.get("repo") ?? "", query.get("query") ?? ""]).then((out) => send(res, 200, out), error(res));
        if (req.method === "GET" && path === "/issues/list") return void o(["issues", "list", query.get("repo") ?? ""]).then((out) => send(res, 200, out), error(res));
        if (req.method === "POST" && path === "/issues/record") return void body(req, 1_048_576).then((input) => oWithInput(["issues", "record", "-"], input, 60_000)).then((out) => send(res, 200, out), error(res));
        if (req.method === "POST" && path === "/issues/resolve") {
          return void body(req, 65_536).then((input) => {
            const data: unknown = JSON.parse(input.toString("utf8"));
            if (!data || typeof data !== "object" || !("id" in data) || !Number.isSafeInteger(data.id) || !("resolution" in data)) throw new Error("발견 ID와 검증 결과가 필요합니다");
            const resolution = data.resolution;
            if (resolution === null) return o(["issues", "reopen", String(data.id)]);
            if (!resolution || typeof resolution !== "object" || !("runId" in resolution) || !("cardId" in resolution) || !("evidence" in resolution) || typeof resolution.runId !== "string" || typeof resolution.cardId !== "string" || typeof resolution.evidence !== "string") throw new Error("검증 결과가 올바르지 않습니다");
            return o(["issues", "resolve", String(data.id), resolution.runId, resolution.cardId, resolution.evidence]);
          }).then((out) => send(res, 200, out), error(res));
        }
        if (req.method === "POST" && path === "/issues/classify") {
          return void body(req, 65_536).then((input) => {
            const data: unknown = JSON.parse(input.toString("utf8"));
            if (!data || typeof data !== "object") throw new Error("분류가 올바르지 않습니다");
            const { id, classification, issue } = data as { id?: unknown; classification?: unknown; issue?: unknown };
            if (!Number.isSafeInteger(id) || typeof classification !== "string") throw new Error("발견 ID와 분류가 필요합니다");
            const args = ["issues", "classify", String(id), classification];
            if (issue !== null && issue !== undefined) {
              if (typeof issue !== "object" || !("number" in issue) || !("updatedAt" in issue) || !Number.isSafeInteger(issue.number) || typeof issue.updatedAt !== "string") throw new Error("이슈 참조가 올바르지 않습니다");
              args.push(String(issue.number), issue.updatedAt);
            }
            return o(args);
          }).then((out) => send(res, 200, out), error(res));
        }
        if (req.method === "POST" && path === "/review") {
          // `o review <pr> --post --detach` in the project folder: reads the PR with gh, starts the review card, returns.
          const project = query.get("project") ?? "";
          const pr = query.get("pr") ?? "";
          if (!PR_SELECTOR.test(pr) || !ABSOLUTE.test(project) || !existsSync(project)) {
            return send(res, 400, JSON.stringify({ error: "PR 번호(또는 github.com PR 주소)와 프로젝트 폴더가 필요합니다" }));
          }
          return void o(["review", pr, "--post", "--detach", "--profile", query.get("clean") === "1" ? "project-o" : "own"], 120_000, project).then((out) => send(res, 200, out), error(res));
        }
        if (req.method === "GET" && path === "/tokens") {
          const project = query.get("project");
          return void o(["tokens", "--json", ...(project ? ["--project", project] : [])]).then((out) => send(res, 200, out), error(res));
        }
        if (path === "/projects" || path?.startsWith("/projects/")) {
          return void projects(req, res, path, query).then((handled) => (handled ? undefined : next()), error(res));
        }
        if (path?.startsWith("/graph/")) {
          return void graph(req, res, path, query).then((handled) => (handled ? undefined : next()), error(res));
        }
        if (path?.startsWith("/dream/") && dream(req, res, path, query)) return;
        if (path?.startsWith("/lead/")) {
          return void lead(req, res, path, query).then((handled) => (handled ? undefined : next()), error(res));
        }
        const action = req.method === "POST" ? AGENT_ACTION.exec(path ?? "") : null;
        if (action) {
          const [, id, verb] = action;
          const args = verb === "install" ? ["agents", "install", id, "--json"] : verb === "setup" ? ["agents", "setup", id, "--json"] : ["agents", "open-login", id, "--json", ...clean];
          // The sandbox check (setup up to 5 minutes, then a probe card up to 2) needs as long as an install.
          return void o(args, verb === "login" ? 60_000 : INSTALL_TIMEOUT_MS).then((out) => send(res, 200, out), error(res));
        }
        next();
      });
    },
  };
}
