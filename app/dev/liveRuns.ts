import { existsSync, readFileSync } from "node:fs";
import type { IncomingMessage, ServerResponse } from "node:http";
import { join } from "node:path";

import type { Plugin } from "vite";

import type { OEvent } from "../../src/core/events.ts";
import { defaultHome, listRuns, runDir } from "../../src/cli/store.ts";
import { loadUsage, readUsage, saveUsage, type UsageReading } from "../../src/cli/usage.ts";

export interface LiveRun {
  runId: string;
  events: OEvent[];
}

/**
 * events.jsonl lines, skipping any that do not parse. A run that is still going
 * can be read between O's append and its newline, so the last line may be half written.
 */
export function parseEventLines(text: string): OEvent[] {
  const events: OEvent[] = [];
  for (const line of text.split("\n")) {
    if (!line.trim()) continue;
    try {
      events.push(JSON.parse(line) as OEvent);
    } catch {
      // Half-written line; the next poll reads it whole.
    }
  }
  return events;
}

/** Runs with a start event, newest first by that start (run ids sort the same way, but the event is the record). */
export function newestFirst(runs: LiveRun[]): LiveRun[] {
  const startOf = (run: LiveRun) => (run.events[0]?.t === "run.start" ? run.events[0].at : "");
  return runs.filter((r) => startOf(r)).sort((a, b) => startOf(b).localeCompare(startOf(a)) || b.runId.localeCompare(a.runId));
}

/** Every run in O's own runs folder. The folder is fixed (defaultHome); requests cannot name another path. */
function readRuns(home: string): LiveRun[] {
  return newestFirst(
    listRuns(home).map((runId) => {
      const path = join(runDir(home, runId), "events.jsonl");
      return { runId, events: existsSync(path) ? parseEventLines(readFileSync(path, "utf8")) : [] };
    }),
  );
}

const EMPTY_USAGE: UsageReading = { at: "", windows: [], unavailable: [] };

function send(res: ServerResponse, status: number, body: unknown) {
  res.statusCode = status;
  res.setHeader("Content-Type", "application/json; charset=utf-8");
  res.setHeader("Cache-Control", "no-store");
  res.end(JSON.stringify(body));
}

/**
 * Only this machine's own address may ask. This middleware runs before Vite's host
 * check, so without it a site using DNS rebinding could read the run records.
 */
function localHost(req: IncomingMessage): boolean {
  return /^(localhost|127\.0\.0\.1|\[::1\])(:\d+)?$/i.test(req.headers.host ?? "");
}

/** A page on another site must not be able to make this machine run the usage CLIs. */
function sameOrigin(req: IncomingMessage): boolean {
  const origin = req.headers.origin;
  return !origin || origin === `http://${req.headers.host}`;
}

/**
 * Dev-server stand-in for the desktop shell (ADR-0001 pending): read-only runs and
 * usage from this machine's O home, and the same usage refresh `o usage` does
 * (official CLIs only, no credential files).
 */
export function liveRuns(): Plugin {
  const home = defaultHome();
  let refreshing: Promise<UsageReading> | undefined;
  return {
    name: "project-o-live-runs",
    apply: "serve",
    configureServer(server) {
      server.middlewares.use("/api", (req, res, next) => {
        const path = req.url?.split("?")[0];
        if (!localHost(req)) return send(res, 403, { error: "이 PC의 주소로만 열 수 있습니다" });
        try {
          if (req.method === "GET" && path === "/runs") return send(res, 200, { runs: readRuns(home) });
          if (req.method === "GET" && path === "/usage") return send(res, 200, loadUsage(home) ?? EMPTY_USAGE);
          if (req.method === "POST" && path === "/usage/refresh") {
            if (!sameOrigin(req)) return send(res, 403, { error: "다른 출처의 요청은 받지 않습니다" });
            refreshing ??= readUsage()
              .then((reading) => (saveUsage(home, reading), reading))
              .finally(() => (refreshing = undefined));
            refreshing.then(
              (reading) => send(res, 200, reading),
              (error: Error) => send(res, 500, { error: error.message }),
            );
            return;
          }
        } catch (error) {
          return send(res, 500, { error: (error as Error).message });
        }
        next();
      });
    },
  };
}
