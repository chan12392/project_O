#!/usr/bin/env node
import { spawn, spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import type { OEvent } from "../core/events.ts";
import { checkPlan, type Violation } from "../core/policy.ts";
import { snapshot } from "../core/snapshot.ts";
import { runPlan } from "./runner.ts";
import {
  activeMarkers,
  createRun,
  defaultHome,
  listRuns,
  newRunId,
  policyContext,
  readEvents,
  readPlan,
  recordBlocked,
  requestStop,
  runDir,
} from "./store.ts";

const USAGE = `O CLI — 작업카드를 공식 에이전트 CLI에 병렬로 전달합니다.

  o check [plan.json|-]      정책 검사만 (실행 안 함)
  o dispatch [plan.json|-]   정책 검사 후 백그라운드 실행, runId 출력
  o status [runId]           카드별 상태 (runId 없으면 최근 5개 실행)
  o events <runId> [--follow]  정규화된 이벤트(JSONL) — UI·총괄 에이전트용
  o stop <runId>             실행 중지

정책 위반이 하나라도 있으면 계획 전체를 실행하지 않고 종료 코드 2로 끝납니다.`;

const home = defaultHome();

async function main(argv: string[]): Promise<number> {
  const [command, ...rest] = argv;
  switch (command) {
    case "check":
      return check(readInput(rest[0]), false);
    case "dispatch":
      return check(readInput(rest[0]), true);
    case "status":
      return status(rest[0]);
    case "events":
      return events(required(rest[0]), rest.includes("--follow"));
    case "stop":
      return stop(required(rest[0]));
    case "_run":
      await runPlan(home, required(rest[0]), readPlan(home, required(rest[0])));
      return 0;
    default:
      console.log(USAGE);
      return command && command !== "help" && command !== "--help" ? 1 : 0;
  }
}

function check(input: unknown, run: boolean): number {
  const now = new Date();
  const { plan, violations } = checkPlan(input, policyContext(home, now));
  if (!plan) {
    if (run) recordBlocked(home, now, (input as { title?: unknown } | null)?.title, violations);
    printViolations(violations);
    return 2;
  }
  if (!run) {
    console.log(`정책 통과: 카드 ${plan.cards.length}개 (${plan.cards.map((c) => `${c.id}→${c.provider}`).join(", ")})`);
    return 0;
  }
  const runId = newRunId(now);
  createRun(home, runId, plan);
  const self = fileURLToPath(import.meta.url);
  const child = spawn(process.execPath, [self, "_run", runId], { detached: true, stdio: "ignore", windowsHide: true });
  child.unref();
  console.log(JSON.stringify({ runId, events: join(runDir(home, runId), "events.jsonl"), cards: plan.cards.map((c) => c.id) }));
  return 0;
}

function printViolations(violations: Violation[]): void {
  console.error("차단됨 — 정책 위반으로 이 계획은 실행되지 않았습니다:");
  for (const v of violations) console.error(`  - [${v.rule}]${v.cardId ? ` (${v.cardId})` : ""} ${v.message}`);
  console.error("위반을 우회하지 말고 사용자에게 이 내용을 그대로 보고하세요.");
}

function status(runId: string | undefined): number {
  const ids = runId ? [runId] : listRuns(home).slice(-5);
  if (ids.length === 0) console.log("실행 기록이 없습니다.");
  for (const id of ids) {
    const run = snapshot(readEvents(home, id));
    if (!run) continue;
    console.log(`${id}  ${run.title}  [${run.status}]`);
    for (const c of run.cards) {
      const activity = c.activity.replace(/\s+/g, " ").slice(0, 100);
      console.log(`  ${c.state.padEnd(8)} ${c.id} (${c.provider} · ${c.role}) ${activity}`);
    }
  }
  return 0;
}

async function events(runId: string, follow: boolean): Promise<number> {
  let printed = 0;
  for (;;) {
    const list = readEvents(home, runId);
    for (const e of list.slice(printed)) console.log(JSON.stringify(e));
    printed = list.length;
    if (!follow || list.some((e) => e.t === "run.end")) return 0;
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
}

function stop(runId: string): number {
  if (!existsSync(runDir(home, runId))) {
    console.error(`실행을 찾지 못했습니다: ${runId}`);
    return 1;
  }
  requestStop(home, runId);
  for (const marker of activeMarkers(home).filter((m) => m.runId === runId)) {
    if (process.platform === "win32") spawnSync("taskkill", ["/PID", String(marker.pid), "/T", "/F"], { windowsHide: true });
    else process.kill(marker.pid, "SIGTERM");
  }
  console.log(`중지 요청: ${runId}`);
  return 0;
}

function readInput(path: string | undefined): unknown {
  const text = !path || path === "-" ? readFileSync(0, "utf8") : readFileSync(path, "utf8");
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

function required(value: string | undefined): string {
  if (!value) {
    console.error(USAGE);
    process.exit(1);
  }
  return value;
}

process.exitCode = await main(process.argv.slice(2));
