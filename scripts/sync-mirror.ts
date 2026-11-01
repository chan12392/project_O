#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { appendFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";

import { defaultHome } from "../src/cli/store.ts";
import { READ_ONLY_BRANCHES } from "../src/guard/rules.ts";

/**
 * Brings a test/main mirror checkout up to its remote branch. Run by the
 * scheduled task from scripts/install-mirror-sync.ps1.
 *
 * Fast-forward only. A mirror with local changes is left alone and logged:
 * mirrors should never have any, and discarding someone's edits silently
 * would be worse than a stale mirror.
 */
const mirror = process.argv[2];
const log = (message: string) => {
  const home = defaultHome();
  mkdirSync(home, { recursive: true });
  appendFileSync(join(home, "mirror-sync.log"), `${new Date().toISOString()} ${mirror} ${message}\n`);
};
const git = (...args: string[]) =>
  execFileSync("git", ["-C", mirror, ...args], { encoding: "utf8", windowsHide: true, stdio: ["ignore", "pipe", "pipe"] }).trim();

try {
  if (!mirror) throw new Error("사용법: node scripts/sync-mirror.ts <mirror 폴더>");
  const branch = git("branch", "--show-current");
  if (!READ_ONLY_BRANCHES.includes(branch)) throw new Error(`거울 브랜치(${READ_ONLY_BRANCHES.join(", ")})가 아닙니다: ${branch}`);
  git("fetch", "--prune", "origin", branch);
  const before = git("rev-parse", "--short", "HEAD");
  const target = git("rev-parse", "--short", `origin/${branch}`);
  if (before === target) process.exit(0);
  if (git("status", "--porcelain")) throw new Error(`로컬 변경이 있어 동기화하지 않았습니다 (${before} → ${target} 대기)`);
  git("merge", "--ff-only", `origin/${branch}`);
  log(`updated ${before} → ${target}`);
} catch (error) {
  log(`error ${(error as Error).message.split("\n")[0]}`);
  process.exitCode = 1;
}
