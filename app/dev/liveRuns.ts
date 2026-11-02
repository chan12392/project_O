import { execFile, spawn } from "node:child_process";
import { existsSync } from "node:fs";
import type { IncomingMessage, ServerResponse } from "node:http";
import { fileURLToPath } from "node:url";

import type { Plugin } from "vite";
import { DESKTOP_UPDATES_ONLY } from "../src/platform/updates.ts";

/**
 * The `o` this checkout builds (`cargo build --release`), else the one on PATH.
 * The dev server reads runs and usage only through `o`, the same code the CLI
 * and the desktop shell use; it never opens O's folder itself.
 */
function oBinary(): string {
  const built = fileURLToPath(new URL(`../../target/release/o${process.platform === "win32" ? ".exe" : ""}`, import.meta.url));
  return existsSync(built) ? built : "o";
}

/** Runs `o` with fixed arguments (no shell) and returns its stdout. */
function o(args: string[], timeout = 60_000, cwd?: string): Promise<string> {
  return new Promise((resolve, reject) =>
    execFile(oBinary(), args, { windowsHide: true, maxBuffer: 256 * 1024 * 1024, timeout, cwd }, (error, stdout, stderr) =>
      error ? reject(new Error(stderr.trim() || error.message)) : resolve(stdout),
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
      const stderr = Buffer.concat(err).toString("utf8").trim();
      if (code === 0) resolve(stdout);
      else reject(Object.assign(new Error(stderr || `o exited with ${code}`), { code }));
    });
    child.stdin.end(input);
  });
}

/** A request body up to `limit` bytes; larger ones are refused before they are read in full. */
function body(req: IncomingMessage, limit: number): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    const parts: Buffer[] = [];
    let size = 0;
    req.on("data", (b: Buffer) => {
      size += b.length;
      if (size > limit) {
        reject(new Error("첨부 파일은 하나에 10MB까지입니다"));
        req.destroy();
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
  return !origin || origin === `http://${req.headers.host}`;
}

/** `/agents/<id>/<action>` with a known agent id, the only ones `o agents` takes. */
const AGENT_ACTION = /^\/agents\/(claude|codex|agy)\/(install|login)$/;

/** A PR number or github.com pull address, as `o review` takes it (platform/backend PR_SELECTOR). */
const PR_SELECTOR = /^(?:[1-9]\d{0,6}|https:\/\/github\.com\/[\w.-]+\/[\w.-]+\/pull\/[1-9]\d{0,6})$/;
/** An absolute folder; `o review`'s policy check decides whether it may be a project. */
const ABSOLUTE = /^(?:[A-Za-z]:[\\/]|\/)/;
/** Settings → 에이전트 → 하루 사용 예산: the day's allowance, percent of the weekly window. */
const BUDGET_PERCENT = /^1[234]$/;

/** npm installs can take minutes. */
const INSTALL_TIMEOUT_MS = 10 * 60_000;
/** A lead turn reads the project before it answers. */
const LEAD_TIMEOUT_MS = 15 * 60_000;
/** One attachment (10MB), with room for nothing else. */
const ATTACH_LIMIT = 10 * 1024 * 1024;
/** Lead ids, efforts and plan ids as `o lead` takes them; it checks every one again. */
const LEAD_PROVIDER = /^(claude|codex)$/;
const LEAD_EFFORT = /^(minimal|low|medium|high|xhigh|max)$/;
const PLAN_ID = /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}-p\d{1,4}$/;

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
  if (req.method === "GET" && path === "/lead/show") {
    run(o(["lead", "show", "--json", ...(projectOk ? ["--project", project] : [])]));
    return true;
  }
  if (req.method !== "POST" || !path.startsWith("/lead/")) return false;
  if (path === "/lead/stop") {
    run(o(["lead", "stop", ...(projectOk ? ["--project", project] : [])]));
    return true;
  }
  if (path === "/lead/dispatch") {
    const plan = query.get("plan") ?? "";
    if (!PLAN_ID.test(plan)) {
    fail(400, "계획 id 형식이 아닙니다");
    return true;
  }
    run(oWithInput(["lead", "dispatch", plan, ...(query.get("clean") === "1" ? ["--clean"] : [])], Buffer.alloc(0), 60_000));
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
    const name = query.get("name") ?? "";
    if (!name) {
    fail(400, "파일 이름이 필요합니다");
    return true;
  }
    try {
      const bytes = await body(req, ATTACH_LIMIT);
      run(oWithInput(["lead", "attach", "--project", project, "--name", name, "-"], bytes, 60_000));
    } catch (e) {
      fail(413, (e as Error).message);
    }
    return true;
  }
  if (path === "/lead/send") {
    let input: { provider?: unknown; effort?: unknown; text?: unknown; attachments?: unknown; clean?: unknown };
    try {
      input = JSON.parse((await body(req, 256 * 1024)).toString("utf8"));
    } catch {
      {
    fail(400, "요청 본문이 JSON이 아닙니다");
    return true;
  }
    }
    const provider = typeof input.provider === "string" && LEAD_PROVIDER.test(input.provider) ? input.provider : "";
    const effort = typeof input.effort === "string" && LEAD_EFFORT.test(input.effort) ? ["--effort", input.effort] : [];
    const files = Array.isArray(input.attachments) ? input.attachments.filter((a): a is string => typeof a === "string" && ABSOLUTE.test(a)) : [];
    if (!provider || typeof input.text !== "string" || !input.text.trim()) {
    fail(400, "에이전트와 보낼 내용이 필요합니다");
    return true;
  }
    const args = ["lead", "send", "--project", project, "--provider", provider, ...effort, ...files.flatMap((f) => ["--attach", f])];
    run(oWithInput([...args, ...(input.clean === true ? ["--clean"] : []), "-"], Buffer.from(input.text, "utf8"), LEAD_TIMEOUT_MS));
    return true;
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
      server.middlewares.use("/api", (req, res, next) => {
        const path = req.url?.split("?")[0];
        if (!localHost(req)) return send(res, 403, JSON.stringify({ error: "이 PC의 주소로만 열 수 있습니다" }));
        if (!sameOrigin(req)) return send(res, 403, JSON.stringify({ error: "다른 출처의 요청은 받지 않습니다" }));
        // ?sessions=1: this PC's recent Claude Code sessions too (`o runs --json --sessions`, read-only).
        const withSessions = new URLSearchParams(req.url?.split("?")[1] ?? "").get("sessions") === "1" ? ["--sessions"] : [];
        if (req.method === "GET" && path === "/runs") return void o(["runs", "--json", ...withSessions]).then((out) => send(res, 200, out), error(res));
        if (req.method === "GET" && path === "/usage") return void o(["usage", "--cached", "--json"]).then((out) => send(res, 200, out), error(res));
        if (req.method === "POST" && path === "/usage/refresh") {
          refreshing ??= o(["usage", "--json"]).finally(() => (refreshing = undefined));
          return void refreshing.then((out) => send(res, 200, out), error(res));
        }
        const query = new URLSearchParams(req.url?.split("?")[1] ?? "");
        if (req.method === "GET" && path === "/budget") return void o(["budget", "--json", "--cached"]).then((out) => send(res, 200, out), error(res));
        if (req.method === "POST" && path === "/budget/percent") {
          // `o budget` checks the range again; only these three ever reach it.
          const percent = query.get("value") ?? "";
          if (!BUDGET_PERCENT.test(percent)) return send(res, 400, JSON.stringify({ error: "하루 사용 예산은 12, 13, 14 중 하나입니다" }));
          return void o(["budget", "--json", "--cached", "--percent", percent]).then((out) => send(res, 200, out), error(res));
        }
        const clean = query.get("clean") === "1" ? ["--clean"] : [];
        if (req.method === "GET" && path === "/agents") return void o(["agents", "status", "--json", ...clean]).then((out) => send(res, 200, out), error(res));
        if (req.method === "GET" && path === "/about") return void o(["about", "--json"]).then((out) => send(res, 200, JSON.stringify({ ...JSON.parse(out), commit: "", builtAt: "" })), error(res));
        if (req.method === "GET" && path === "/updates") return send(res, 200, JSON.stringify({ release: null, message: DESKTOP_UPDATES_ONLY }));
        if (req.method === "POST" && path === "/review") {
          // `o review <pr> --post --detach` in the project folder: reads the PR with gh, starts the review card, returns.
          const project = query.get("project") ?? "";
          const pr = query.get("pr") ?? "";
          if (!PR_SELECTOR.test(pr) || !ABSOLUTE.test(project) || !existsSync(project)) {
            return send(res, 400, JSON.stringify({ error: "PR 번호(또는 github.com PR 주소)와 프로젝트 폴더가 필요합니다" }));
          }
          return void o(["review", pr, "--post", "--detach", ...clean], 120_000, project).then((out) => send(res, 200, out), error(res));
        }
        if (req.method === "GET" && path === "/tokens") {
          const project = query.get("project");
          return void o(["tokens", "--json", ...(project ? ["--project", project] : [])]).then((out) => send(res, 200, out), error(res));
        }
        if (path === "/projects" || path?.startsWith("/projects/")) {
          return void projects(req, res, path, query).then((handled) => (handled ? undefined : next()), error(res));
        }
        if (path?.startsWith("/lead/")) {
          return void lead(req, res, path, query).then((handled) => (handled ? undefined : next()), error(res));
        }
        const action = req.method === "POST" ? AGENT_ACTION.exec(path ?? "") : null;
        if (action) {
          const [, id, verb] = action;
          const args = verb === "install" ? ["agents", "install", id, "--json"] : ["agents", "open-login", id, "--json", ...clean];
          return void o(args, verb === "install" ? INSTALL_TIMEOUT_MS : 60_000).then((out) => send(res, 200, out), error(res));
        }
        next();
      });
    },
  };
}
