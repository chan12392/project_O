#!/usr/bin/env node
import { spawn, spawnSync } from "node:child_process";
import { existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import type { OEvent } from "../core/events.ts";
import { checkMessage, checkPlan, type Violation } from "../core/policy.ts";
import { outboxPath } from "../core/providers.ts";
import { snapshot } from "../core/snapshot.ts";
import { runPlan } from "./runner.ts";
import { readUsage, saveUsage } from "./usage.ts";
import {
  activeMarkers,
  appendEvent,
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
  o msg send [--to <카드|lead|all>] "내용"   팀 대화에 보내기 (카드 안에서는 실행·카드가 자동)
  o msg read [--all]         나에게 온 새 메시지 (--all: 전체 대화)
  o msg post                 카드의 outbox 파일(첫 줄 "to: <상대>")을 팀 대화로 보내기
                             카드 밖에서는 --run <runId> 와 --as <카드|lead> 로 지정
  o usage [--json]           Codex·Antigravity 구독 한도를 각 CLI로 조회해 usage.json에 저장

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
    case "msg":
      return message(rest);
    case "usage":
      return usage(rest.includes("--json"));
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
      console.log(`  ${c.state.padEnd(8)} ${c.id} (${c.provider} · ${c.label}) ${activity}`);
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

/**
 * The run's team channel. Cards get O_RUN_ID and O_CARD_ID from the runner;
 * the lead or a person passes --run and --as. Messages are events in the
 * run's events.jsonl, checked by the same policy rules as tasks.
 */
function message(args: string[]): number {
  const [verb, ...rest] = args;
  const flags = new Map<string, string>();
  const words: string[] = [];
  for (let i = 0; i < rest.length; i += 1) {
    if (rest[i] === "--all") flags.set("all", "1");
    else if (rest[i].startsWith("--")) flags.set(rest[i].slice(2), rest[(i += 1)] ?? "");
    else words.push(rest[i]);
  }
  const runId = flags.get("run") ?? process.env.O_RUN_ID;
  const me = flags.get("as") ?? process.env.O_CARD_ID ?? "lead";
  if (!runId || !existsSync(runDir(home, runId))) {
    console.error(`실행을 찾지 못했습니다: ${runId ?? "(--run 없음)"}`);
    return 1;
  }
  const cards = readPlan(home, runId).cards.map((c) => c.id);

  const send = (to: string, text: string): number => {
    const violations = checkMessage(text, { from: me, to, known: cards });
    if (violations.length > 0) {
      printViolations(violations);
      return 2;
    }
    appendEvent(home, runId, { t: "msg", at: new Date().toISOString(), from: me, to, text });
    console.log(`보냄: ${me} → ${to}`);
    return 0;
  };

  if (verb === "send") return send(flags.get("to") ?? "all", words.join(" "));
  if (verb === "post") {
    // For CLIs that may only run fixed commands: the message waits in the card's outbox file.
    const path = outboxPath(runDir(home, runId), me);
    if (!existsSync(path)) {
      console.error(`보낼 메시지 파일이 없습니다: ${path}`);
      return 1;
    }
    const [first = "", ...lines] = readFileSync(path, "utf8").split(/\r?\n/);
    const to = /^to:\s*(\S+)/i.exec(first)?.[1];
    const text = (to ? lines : [first, ...lines]).join("\n").trim();
    const code = send(to ?? "all", text);
    if (code === 0) rmSync(path);
    return code;
  }
  if (verb === "read") {
    const all = readEvents(home, runId).filter((e) => e.t === "msg");
    const mine = flags.has("all") ? all : all.filter((e) => e.t === "msg" && e.from !== me && (e.to === me || e.to === "all"));
    const cursorPath = join(runDir(home, runId), `read.${me}`);
    const seen = flags.has("all") || !existsSync(cursorPath) ? 0 : Number(readFileSync(cursorPath, "utf8")) || 0;
    const fresh = mine.slice(seen);
    if (!flags.has("all")) writeFileSync(cursorPath, String(mine.length));
    if (fresh.length === 0) console.log("새 메시지가 없습니다.");
    for (const e of fresh) if (e.t === "msg") console.log(`[${e.from} → ${e.to}] ${e.text}`);
    return 0;
  }
  console.error(USAGE);
  return 1;
}

async function usage(json: boolean): Promise<number> {
  const reading = await readUsage();
  saveUsage(home, reading);
  if (json) {
    console.log(JSON.stringify(reading, null, 2));
    return 0;
  }
  for (const w of reading.windows) {
    const name = w.window === "five_hour" ? "5시간" : w.window === "seven_day" ? "주간" : w.window;
    console.log(`${w.provider.padEnd(6)} ${(w.group ?? "").padEnd(22)} ${name.padEnd(4)} ${String(Math.round(w.used * 100)).padStart(3)}% 사용  ${w.resetsAt} 초기화`);
  }
  for (const u of reading.unavailable) console.log(`${u.provider.padEnd(6)} 조회 실패: ${u.reason}`);
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
