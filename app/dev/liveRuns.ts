import { execFile } from "node:child_process";
import { existsSync } from "node:fs";
import type { IncomingMessage, ServerResponse } from "node:http";
import { fileURLToPath } from "node:url";

import type { Plugin } from "vite";

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
function o(args: string[], timeout = 60_000): Promise<string> {
  return new Promise((resolve, reject) =>
    execFile(oBinary(), args, { windowsHide: true, maxBuffer: 256 * 1024 * 1024, timeout }, (error, stdout, stderr) =>
      error ? reject(new Error(stderr.trim() || error.message)) : resolve(stdout),
    ),
  );
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

/** Settings → 에이전트 → 하루 사용 예산: the day's allowance, percent of the weekly window. */
const BUDGET_PERCENT = /^1[234]$/;

/** npm installs can take minutes. */
const INSTALL_TIMEOUT_MS = 10 * 60_000;

/**
 * Dev-server stand-in for the desktop shell (backend.ts, useLiveRuns.ts): runs, usage,
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
        if (req.method === "GET" && path === "/runs") return void o(["runs", "--json"]).then((out) => send(res, 200, out), error(res));
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
        if (req.method === "GET" && path === "/about") return void o(["about", "--json"]).then((out) => send(res, 200, out), error(res));
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
