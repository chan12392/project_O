import { appendFileSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

import type { OEvent } from "../core/events.ts";
import type { Plan, ProviderId } from "../core/plan.ts";
import { LIMITS, emptyCounts, type PolicyContext, type Violation } from "../core/policy.ts";
import { realGit } from "../guard/hook.ts";

/**
 * O's state directory. Fixed per machine on purpose: if a caller could point
 * O at another directory, it could also escape the cooldowns and caps kept
 * here. Tests pass their own `home` to these functions instead.
 *
 *   runs/<runId>/plan.json · events.jsonl · cards/<id>.raw.jsonl · stop
 *   active/<runId>.<cardId>.json   running card markers ({ pid, provider })
 *   ledger.jsonl                   one line per started card (daily cap)
 *   cooldowns.json                 provider → ISO time it is blocked until
 *   blocked.jsonl                  plans the policy refused (shown in the UI)
 */
export function defaultHome(): string {
  const base = process.env.LOCALAPPDATA ?? join(homedir(), ".local", "state");
  return join(base, "project_O");
}

export function runDir(home: string, runId: string): string {
  return join(home, "runs", runId);
}

export function newRunId(now: Date): string {
  const stamp = now.toISOString().replace(/[-:]/g, "").replace(/\..+/, "").replace("T", "-");
  return `${stamp}-${Math.random().toString(36).slice(2, 6)}`;
}

export function createRun(home: string, runId: string, plan: Plan): void {
  mkdirSync(join(runDir(home, runId), "cards"), { recursive: true });
  writeFileSync(join(runDir(home, runId), "plan.json"), JSON.stringify(plan, null, 2));
}

export function readPlan(home: string, runId: string): Plan {
  return JSON.parse(readFileSync(join(runDir(home, runId), "plan.json"), "utf8")) as Plan;
}

export function appendEvent(home: string, runId: string, event: OEvent): void {
  appendFileSync(join(runDir(home, runId), "events.jsonl"), `${JSON.stringify(event)}\n`);
}

export function readEvents(home: string, runId: string): OEvent[] {
  const path = join(runDir(home, runId), "events.jsonl");
  if (!existsSync(path)) return [];
  return readLines(path).map((line) => JSON.parse(line) as OEvent);
}

export function listRuns(home: string): string[] {
  const dir = join(home, "runs");
  if (!existsSync(dir)) return [];
  return readdirSync(dir).sort();
}

export interface ActiveMarker {
  runId: string;
  cardId: string;
  provider: ProviderId;
  pid: number;
}

export function markActive(home: string, marker: ActiveMarker): void {
  mkdirSync(join(home, "active"), { recursive: true });
  writeFileSync(join(home, "active", `${marker.runId}.${marker.cardId}.json`), JSON.stringify(marker));
}

export function clearActive(home: string, runId: string, cardId: string): void {
  rmSync(join(home, "active", `${runId}.${cardId}.json`), { force: true });
}

/** Live markers only: a marker whose process is gone is stale and ignored. */
export function activeMarkers(home: string): ActiveMarker[] {
  const dir = join(home, "active");
  if (!existsSync(dir)) return [];
  const out: ActiveMarker[] = [];
  for (const name of readdirSync(dir)) {
    try {
      const marker = JSON.parse(readFileSync(join(dir, name), "utf8")) as ActiveMarker;
      if (isAlive(marker.pid)) out.push(marker);
    } catch {
      // Half-written marker from a crashed runner; it no longer counts.
    }
  }
  return out;
}

export function recordStart(home: string, runId: string, cardId: string, provider: ProviderId, at: Date): void {
  mkdirSync(home, { recursive: true });
  appendFileSync(join(home, "ledger.jsonl"), `${JSON.stringify({ at: at.toISOString(), runId, cardId, provider })}\n`);
}

export function startedOn(home: string, day: Date): number {
  const path = join(home, "ledger.jsonl");
  if (!existsSync(path)) return 0;
  const prefix = localDay(day);
  return readLines(path).filter((line) => localDay(new Date(JSON.parse(line).at)) === prefix).length;
}

export function readCooldowns(home: string): Partial<Record<ProviderId, string>> {
  const path = join(home, "cooldowns.json");
  if (!existsSync(path)) return {};
  return JSON.parse(readFileSync(path, "utf8"));
}

/** Records a provider limit. An empty `until` means the CLI gave no reset time. */
export function setCooldown(home: string, provider: ProviderId, until: string, now: Date): string {
  const resolved = until || new Date(now.getTime() + LIMITS.defaultCooldownMs).toISOString();
  const cooldowns = readCooldowns(home);
  const current = cooldowns[provider];
  if (!current || Date.parse(current) < Date.parse(resolved)) cooldowns[provider] = resolved;
  mkdirSync(home, { recursive: true });
  writeFileSync(join(home, "cooldowns.json"), JSON.stringify(cooldowns, null, 2));
  return cooldowns[provider] as string;
}

export function recordBlocked(home: string, at: Date, title: unknown, violations: Violation[]): void {
  mkdirSync(home, { recursive: true });
  appendFileSync(join(home, "blocked.jsonl"), `${JSON.stringify({ at: at.toISOString(), title, violations })}\n`);
}

export function policyContext(home: string, now: Date): PolicyContext {
  const activeByProvider = emptyCounts();
  for (const marker of activeMarkers(home)) activeByProvider[marker.provider] += 1;
  return {
    now,
    activeByProvider,
    startedToday: startedOn(home, now),
    cooldowns: readCooldowns(home),
    projectExists: (path) => existsSync(path),
    isMirror: realGit.isMirror,
  };
}

export function requestStop(home: string, runId: string): void {
  writeFileSync(join(runDir(home, runId), "stop"), "");
}

export function stopRequested(home: string, runId: string): boolean {
  return existsSync(join(runDir(home, runId), "stop"));
}

function readLines(path: string): string[] {
  return readFileSync(path, "utf8").split("\n").filter((line) => line.trim());
}

function localDay(date: Date): string {
  return `${date.getFullYear()}-${date.getMonth() + 1}-${date.getDate()}`;
}

function isAlive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return (error as NodeJS.ErrnoException).code === "EPERM";
  }
}
