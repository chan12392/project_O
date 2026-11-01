import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

import { parseAgyUsage, parseCodexRateLimits, type UsageWindow } from "../core/usage.ts";

const TIMEOUT_MS = 20_000;

export interface UsageReading {
  at: string;
  windows: UsageWindow[];
  /** Providers whose CLI could not answer, with why. */
  unavailable: { provider: string; reason: string }[];
}

/** Finds a CLI on PATH; npm installs Codex as a .cmd shim, so we run its script with node. */
function resolveCli(name: string): [string, string[]] | undefined {
  const where = spawnSync(process.platform === "win32" ? "where" : "which", [name], { encoding: "utf8", windowsHide: true });
  const first = where.stdout?.split(/\r?\n/).find(Boolean);
  if (!first) return undefined;
  if (/\.cmd$/i.test(first) || name === "codex") {
    const script = join(dirname(first), "node_modules", "@openai", "codex", "bin", "codex.js");
    if (existsSync(script)) return [process.execPath, [script]];
  }
  return [first, []];
}

/** Codex: a short-lived, read-only app-server answers account/rateLimits/read (no model turn). */
function codexWindows(): Promise<UsageWindow[]> {
  const cli = resolveCli("codex");
  if (!cli) return Promise.reject(new Error("codex CLI를 찾지 못했습니다"));
  return new Promise((resolve, reject) => {
    const child = spawn(cli[0], [...cli[1], "-c", "approval_policy=never", "-c", "features.plugins=false", "-s", "read-only", "-a", "never", "app-server"], {
      windowsHide: true,
      stdio: ["pipe", "pipe", "ignore"],
    });
    let buffer = "";
    const finish = (error?: Error, windows?: UsageWindow[]) => {
      clearTimeout(timer);
      child.kill();
      if (error) reject(error);
      else resolve(windows ?? []);
    };
    const timer = setTimeout(() => finish(new Error("codex app-server가 시간 안에 답하지 않았습니다")), TIMEOUT_MS);
    const send = (message: object) => child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", ...message })}\n`);
    child.on("error", (error) => finish(error));
    child.stdout.on("data", (chunk: Buffer) => {
      buffer += chunk.toString("utf8");
      for (let nl = buffer.indexOf("\n"); nl >= 0; nl = buffer.indexOf("\n")) {
        const line = buffer.slice(0, nl);
        buffer = buffer.slice(nl + 1);
        let message: { id?: number; result?: unknown; error?: { message?: string } };
        try {
          message = JSON.parse(line);
        } catch {
          continue;
        }
        if (message.id === 1) {
          send({ method: "initialized", params: {} });
          send({ id: 2, method: "account/rateLimits/read", params: {} });
        } else if (message.id === 2) {
          if (message.error) finish(new Error(message.error.message ?? "rate limit read failed"));
          else finish(undefined, parseCodexRateLimits(message.result));
        }
      }
    });
    send({ id: 1, method: "initialize", params: { clientInfo: { name: "project-o", version: "0.0.1" } } });
  });
}

/** Antigravity: print-mode /usage reports quota without a model turn (agy 1.1.11 and later). */
function agyWindows(): UsageWindow[] {
  const cli = resolveCli("agy");
  if (!cli) throw new Error("agy CLI를 찾지 못했습니다");
  const result = spawnSync(cli[0], [...cli[1], "-p", "/usage", "--output-format", "json", "--print-timeout", "20s"], {
    encoding: "utf8",
    windowsHide: true,
    timeout: TIMEOUT_MS + 10_000,
    env: { ...process.env, AGY_CLI_DISABLE_AUTO_UPDATE: "true" },
  });
  if (result.status !== 0) throw new Error(`agy /usage 실패 (${result.status ?? "시간 초과"})`);
  return parseAgyUsage(result.stdout ?? "");
}

export async function readUsage(): Promise<UsageReading> {
  const reading: UsageReading = { at: new Date().toISOString(), windows: [], unavailable: [] };
  const [codex] = await Promise.allSettled([codexWindows()]);
  if (codex.status === "fulfilled") reading.windows.push(...codex.value);
  else reading.unavailable.push({ provider: "codex", reason: (codex.reason as Error).message });
  try {
    reading.windows.push(...agyWindows());
  } catch (error) {
    reading.unavailable.push({ provider: "agy", reason: (error as Error).message });
  }
  return reading;
}

export function saveUsage(home: string, reading: UsageReading): void {
  mkdirSync(home, { recursive: true });
  writeFileSync(join(home, "usage.json"), JSON.stringify(reading, null, 2));
}

export function loadUsage(home: string): UsageReading | undefined {
  const path = join(home, "usage.json");
  return existsSync(path) ? (JSON.parse(readFileSync(path, "utf8")) as UsageReading) : undefined;
}
