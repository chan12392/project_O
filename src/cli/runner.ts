import { spawn, type ChildProcess } from "node:child_process";
import { appendFileSync } from "node:fs";
import { join } from "node:path";
import { createInterface } from "node:readline";

import type { CardStatus } from "../core/events.ts";
import { cardLabel, type Card, type Plan, type ProviderId } from "../core/plan.ts";
import { LIMITS } from "../core/policy.ts";
import { StreamReader, launchSpec, type LaunchSpec } from "../core/providers.ts";
import {
  activeMarkers,
  appendEvent,
  clearActive,
  markActive,
  readCooldowns,
  recordStart,
  runDir,
  setCooldown,
  startedOn,
  stopRequested,
} from "./store.ts";
import { resolveCli } from "./resolve.ts";

export type Spawner = (spec: LaunchSpec, cwd: string) => ChildProcess;

const WAIT_FOR_SLOT_MS = 2_000;

/**
 * Runs every card of a plan in parallel, as far as the caps allow, and writes
 * the normalized events. Cards that cannot get a slot wait in a queue; a card
 * whose provider hits a limit mid-run stops its queued siblings on that
 * provider instead of retrying them.
 */
export async function runPlan(home: string, runId: string, plan: Plan, spawner: Spawner = spawnOfficial): Promise<CardStatus> {
  const emit = (event: Parameters<typeof appendEvent>[2]) => appendEvent(home, runId, event);
  const now = () => new Date().toISOString();
  emit({
    t: "run.start",
    at: now(),
    runId,
    title: plan.title,
    project: plan.project,
    cards: plan.cards.map((c) => ({ id: c.id, provider: c.provider, label: cardLabel(c), role: c.role, model: c.model, effort: c.effort })),
  });

  const pending = [...plan.cards];
  const queued = new Set<string>();
  const running = new Map<string, Promise<CardStatus>>();
  const results: CardStatus[] = [];

  while (pending.length > 0 || running.size > 0) {
    if (stopRequested(home, runId)) {
      for (const card of pending.splice(0)) {
        emit({ t: "card.end", at: now(), cardId: card.id, status: "stopped", exitCode: null });
        results.push("stopped");
      }
    }
    for (const card of [...pending]) {
      const until = readCooldowns(home)[card.provider];
      if (until && Date.parse(until) > Date.now()) {
        pending.splice(pending.indexOf(card), 1);
        emit({ t: "card.end", at: now(), cardId: card.id, status: "failed", result: `${card.provider} 사용량 한도로 ${until}까지 차단됨`, exitCode: null });
        results.push("failed");
        continue;
      }
      // Dispatch checked the day's cap, but plans dispatched back to back all see the same count.
      if (startedOn(home, new Date()) >= LIMITS.maxCardsPerDay) {
        pending.splice(pending.indexOf(card), 1);
        emit({ t: "card.end", at: now(), cardId: card.id, status: "failed", result: `오늘 실행 상한 ${LIMITS.maxCardsPerDay}개에 닿아 시작하지 않음`, exitCode: null });
        results.push("failed");
        continue;
      }
      const reason = slotBlocker(home, card.provider);
      if (reason) {
        if (!queued.has(card.id)) emit({ t: "card.queued", at: now(), cardId: card.id, reason });
        queued.add(card.id);
        continue;
      }
      pending.splice(pending.indexOf(card), 1);
      const job = runCard(home, runId, plan, card, spawner).then((status) => {
        running.delete(card.id);
        results.push(status);
        return status;
      });
      running.set(card.id, job);
    }
    if (running.size > 0) await Promise.race(running.values());
    else if (pending.length > 0) await sleep(WAIT_FOR_SLOT_MS);
  }

  const status: CardStatus = results.includes("stopped") ? "stopped" : results.every((r) => r === "done") ? "done" : "failed";
  emit({ t: "run.end", at: now(), runId, status });
  return status;
}

function slotBlocker(home: string, provider: ProviderId): string | undefined {
  const active = activeMarkers(home);
  if (active.length >= LIMITS.maxActiveTotal) return `전체 동시 실행 ${LIMITS.maxActiveTotal}개가 차서 대기`;
  const mine = active.filter((m) => m.provider === provider).length;
  if (mine >= LIMITS.maxActivePerProvider[provider]) return `${provider} 동시 실행 ${LIMITS.maxActivePerProvider[provider]}개가 차서 대기`;
  return undefined;
}

function runCard(home: string, runId: string, plan: Plan, card: Card, spawner: Spawner): Promise<CardStatus> {
  const emit = (event: Parameters<typeof appendEvent>[2]) => appendEvent(home, runId, event);
  const now = () => new Date().toISOString();
  const cardsDir = join(runDir(home, runId), "cards");
  const spec = launchSpec(card, plan, { runId, runDir: runDir(home, runId) });

  let child: ChildProcess;
  try {
    child = spawner(spec, plan.project);
  } catch (error) {
    emit({ t: "card.end", at: now(), cardId: card.id, status: "failed", result: `실행 실패: ${(error as Error).message}`, exitCode: null });
    return Promise.resolve("failed");
  }

  // No pid: the program never started; "error" follows, nothing to count.
  if (child.pid === undefined) {
    return new Promise((resolve) =>
      child.once("error", (error) => {
        emit({ t: "card.end", at: now(), cardId: card.id, status: "failed", result: `실행 실패: ${error.message}`, exitCode: null });
        resolve("failed");
      }),
    );
  }

  // Count the card the moment it holds a slot, so a parallel dispatch sees it.
  markActive(home, { runId, cardId: card.id, provider: card.provider, pid: child.pid });
  recordStart(home, runId, card.id, card.provider, new Date());
  emit({ t: "card.start", at: now(), cardId: card.id, pid: child.pid });

  if (spec.stdin !== undefined) child.stdin?.end(spec.stdin);
  else child.stdin?.end();

  const reader = new StreamReader(card.provider);
  let final: { ok: boolean; text: string } | undefined;
  let lastMessage = "";
  let stderrTail = "";
  child.stderr?.on("data", (chunk: Buffer) => {
    appendFileSync(join(cardsDir, `${card.id}.err.log`), chunk);
    stderrTail = (stderrTail + chunk.toString("utf8")).slice(-400);
  });
  const lines = createInterface({ input: child.stdout! });
  lines.on("line", (line) => {
    appendFileSync(join(cardsDir, `${card.id}.raw.jsonl`), `${line}\n`);
    for (const signal of reader.read(line)) {
      if (signal.kind === "session") emit({ t: "card.session", at: now(), cardId: card.id, sessionId: signal.sessionId });
      else if (signal.kind === "message") {
        lastMessage = signal.text;
        emit({ t: "card.message", at: now(), cardId: card.id, text: signal.text });
      } else if (signal.kind === "tool") emit({ t: "card.tool", at: now(), cardId: card.id, name: signal.name, summary: signal.summary });
      else if (signal.kind === "limit") {
        const until = setCooldown(home, card.provider, signal.until, new Date());
        emit({ t: "card.limit", at: now(), cardId: card.id, until });
      } else if (signal.kind === "usage") emit({ t: "card.usage", at: now(), cardId: card.id, input: signal.input, cached: signal.cached, output: signal.output });
      else if (signal.kind === "quota") emit({ t: "quota", at: now(), provider: card.provider, window: signal.window, used: signal.used, resetsAt: signal.resetsAt });
      else final = { ok: signal.ok, text: signal.text };
    }
  });

  return new Promise((resolve) => {
    let settled = false;
    const finish = (exitCode: number | null, error?: string) => {
      if (settled) return;
      settled = true;
      clearActive(home, runId, card.id);
      const stopped = stopRequested(home, runId);
      const status: CardStatus = stopped ? "stopped" : final?.ok && exitCode === 0 ? "done" : "failed";
      // A failure with nothing on stdout usually explains itself on stderr.
      const result = error ?? (final?.text || lastMessage || (status === "failed" ? stderrTail.trim() : "") || undefined);
      emit({ t: "card.end", at: now(), cardId: card.id, status, result, exitCode });
      resolve(status);
    };
    child.on("error", (error) => finish(null, `실행 실패: ${error.message}`));
    // "close" fires after stdout has ended, so the final line has been read.
    child.on("close", (code) => finish(code));
  });
}

/** Launches the official CLI found on PATH, never through a shell (see resolveCli). */
function spawnOfficial(spec: LaunchSpec, cwd: string): ChildProcess {
  const cli = resolveCli(spec.bin);
  if (!cli) throw new Error(`${spec.bin} CLI를 PATH에서 찾지 못했거나 셸 없이 실행할 수 없습니다`);
  return spawn(cli.command, [...cli.prefix, ...spec.args], { cwd, windowsHide: true, stdio: ["pipe", "pipe", "pipe"], env: { ...process.env, ...spec.env } });
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
