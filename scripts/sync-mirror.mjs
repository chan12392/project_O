#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { appendFileSync, mkdirSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

/**
 * Brings a test/main mirror checkout up to its remote branch. The installer
 * (scripts/install-mirror-sync.ps1) copies this file out of the repo, so it
 * imports nothing from the repo: a mirror that is behind may not contain
 * this script at all.
 *
 * Fast-forward only. A mirror with local changes is left alone and logged:
 * mirrors should never have any, and discarding someone's edits silently
 * would be worse than a stale mirror.
 */
const MIRROR_BRANCHES = ["test", "main"];
const mirror = process.argv[2];
const home = join(process.env.LOCALAPPDATA ?? join(homedir(), ".local", "state"), "project_O");
const log = (message) => {
  mkdirSync(home, { recursive: true });
  appendFileSync(join(home, "mirror-sync.log"), `${new Date().toISOString()} ${mirror} ${message}\n`);
};
// No repository hooks: this runs unattended every few minutes, and a hook merged into
// test would otherwise run here unseen. Pointing hooksPath at a missing folder disables them.
const NO_HOOKS = join(home, "no-hooks");
const git = (...args) =>
  execFileSync("git", ["-c", `core.hooksPath=${NO_HOOKS}`, "-C", mirror, ...args], { encoding: "utf8", windowsHide: true, stdio: ["ignore", "pipe", "pipe"] }).trim();

try {
  if (!mirror) throw new Error("사용법: node sync-mirror.mjs <mirror 폴더>");
  const branch = git("branch", "--show-current");
  if (!MIRROR_BRANCHES.includes(branch)) throw new Error(`거울 브랜치(${MIRROR_BRANCHES.join(", ")})가 아닙니다: ${branch}`);
  git("fetch", "--prune", "origin", branch);
  const before = git("rev-parse", "--short", "HEAD");
  const target = git("rev-parse", "--short", `origin/${branch}`);
  if (before !== target) {
    if (git("status", "--porcelain")) throw new Error(`로컬 변경이 있어 동기화하지 않았습니다 (${before} → ${target} 대기)`);
    git("merge", "--ff-only", `origin/${branch}`);
    log(`updated ${before} → ${target}`);
  }
} catch (error) {
  log(`error ${String(error.message).split("\n")[0]}`);
  process.exitCode = 1;
}
