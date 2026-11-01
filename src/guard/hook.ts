#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, statSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { CODE_RULES, CREDENTIAL_RULES, PROJECT_REMOTE, READ_ONLY_BRANCHES, READ_ONLY_COMMANDS, findHits, isAllowedPath } from "./rules.ts";

/**
 * PreToolUse hook for Claude Code and Codex working in this repository.
 * Exit code 2 blocks the tool call and hands the reason back to the agent.
 *
 * It refuses: shell commands that touch provider credentials or disable a
 * CLI's permission checks, code edits that introduce those, commits or
 * pushes to main, branch names outside the <personal>-<topic> rule, and any
 * change inside a test/main checkout (those are pull-only mirrors).
 */
export interface HookInput {
  tool_name?: string;
  tool_input?: Record<string, unknown>;
  cwd?: string;
}

/** Keep in sync with .github/workflows/branch-flow.yml. */
export const PERSONAL_BRANCHES = ["younchan", "yongwoo"];
const BRANCH_NAME = new RegExp(`^(${PERSONAL_BRANCHES.join("|")})(-[a-z0-9][a-z0-9._-]*)?$`);
const CREATE_BRANCH = /\bgit\s+(?:switch\s+(?:-c|-C|--create)|checkout\s+(?:-b|-B)|branch)\s+([^\s-]\S*)/g;

/** What the hook asks git about a directory. Both answer "" / false outside a repo. */
export interface GitProbe {
  branch: (dir: string) => string;
  /** A project_O checkout on test or main — not any repository that happens to be on main. */
  isMirror: (dir: string) => boolean;
}

const READ_ONLY_REASON = "project_O의 test·main 작업 폴더는 읽기 전용입니다(풀만 받는 거울). 자기 개인 브랜치 폴더에서 작업하고 PR로 올리세요";

export function review(input: HookInput, git: GitProbe): string[] {
  const tool = input.tool_name ?? "";
  const args = input.tool_input ?? {};
  const cwd = input.cwd ?? process.cwd();
  let branch: string | undefined;
  const currentBranch = () => (branch ??= git.branch(cwd));
  const reasons: string[] = [];

  if (tool === "Bash" || tool === "shell" || tool === "PowerShell") {
    const command = String(args.command ?? "");
    for (const hit of findHits(command, CODE_RULES)) reasons.push(hit.reason);
    reasons.push(...gitReasons(command, currentBranch));
    // A mirror is read-only whether the command runs in it or names it from elsewhere.
    if (git.isMirror(cwd) || pathsIn(command).some((p) => git.isMirror(p))) reasons.push(...readOnlyShellReasons(command));
    return reasons;
  }

  // Reading a CLI's sign-in files is as off-limits as copying them in a shell.
  if (tool === "Read" || tool === "Grep" || tool === "Glob") {
    return findHits(JSON.stringify(args), CREDENTIAL_RULES).map((hit) => hit.reason);
  }

  const rawPaths = editedPaths(tool, args);
  for (const path of rawPaths) {
    // Forward slashes so dirname works on Linux too (hosts send Windows paths).
    const absolute = (/^([A-Za-z]:)?[\\/]/.test(path) ? path : `${cwd}/${path}`).replace(/\\/g, "/");
    if (git.isMirror(dirname(absolute))) {
      reasons.push(`${READ_ONLY_REASON} (${path})`);
      return reasons;
    }
  }
  const paths = rawPaths.map((p) => repoRelative(p, cwd));
  if (paths.length > 0 && paths.every((p) => isAllowedPath(p))) return reasons;
  if (paths.length > 0 || tool === "apply_patch") {
    for (const hit of findHits(JSON.stringify(args), CODE_RULES)) reasons.push(`${hit.reason} (${paths.join(", ") || tool})`);
  }
  return reasons;
}

function gitReasons(command: string, currentBranch: () => string): string[] {
  const reasons: string[] = [];
  const segments = command.split(/&&|\|\||;|\|/);
  for (const segment of segments) {
    if (!/\bgit\b/.test(segment)) continue;
    if (/\bpush\b/.test(segment)) {
      if (/(\s|:)(refs\/heads\/)?main\b/.test(segment)) reasons.push("main에는 직접 push할 수 없습니다. test → main PR로 병합하세요");
      else if (/(--force\b|--force-with-lease\b|\s-f\b)/.test(segment) && /\stest\b/.test(segment)) reasons.push("test 브랜치는 강제 push할 수 없습니다");
      else if (!/\bpush\s+\S+\s+\S+/.test(segment) && currentBranch() === "main") reasons.push("main 브랜치에서 push할 수 없습니다");
    }
    if (/\b(commit|merge|cherry-pick|rebase)\b/.test(segment) && currentBranch() === "main") {
      reasons.push("main 브랜치에서는 커밋·병합하지 않습니다. 자기 개인 브랜치나 <개인 브랜치>-<주제> 브랜치로 옮기세요");
    }
  }
  for (const match of command.matchAll(CREATE_BRANCH)) {
    const name = match[1];
    if (!BRANCH_NAME.test(name)) reasons.push(`브랜치 이름 ${name}은 규칙(${PERSONAL_BRANCHES.join(", ")} 또는 <개인 브랜치>-<주제>)에 맞지 않습니다`);
  }
  return reasons;
}

/** Absolute paths and `git -C` targets a command names, so a mirror can't be changed from another folder. */
function pathsIn(command: string): string[] {
  const found = [
    ...[...command.matchAll(/\bgit\s+-C\s+("[^"]+"|'[^']+'|\S+)/g)].map((m) => m[1]),
    ...(command.match(/(?:[A-Za-z]:[\\/]|\/)[^\s"'|;&<>]+/g) ?? []),
  ];
  return [...new Set(found.map((p) => p.replace(/^["']|["']$/g, "")))].slice(0, 6);
}

function readOnlyShellReasons(command: string): string[] {
  const reasons: string[] = [];
  // Substitution or a background `&` would run a second command the allow list never saw.
  if (/\$\(|`|(?<![&>])&(?![&>])/.test(command)) reasons.push(`${READ_ONLY_REASON} — 명령 치환이나 &로 이은 명령은 허용하지 않습니다`);
  // Read-only commands with options that run a program or write a file.
  if (/\s(--pre\b|-O\S*|--open-files-in-pager\b|--output\b)/.test(command)) reasons.push(`${READ_ONLY_REASON} — 다른 프로그램을 실행하거나 파일을 쓰는 옵션은 허용하지 않습니다`);
  for (const raw of command.split(/&&|\|\||;|\||\n/)) {
    // `git -C <mirror> status` is judged as `git status`.
    const segment = raw.trim().replace(/^git\s+-C\s+("[^"]+"|'[^']+'|\S+)\s+/, "git ");
    if (!segment) continue;
    const withoutSafeRedirects = segment.replace(/\d?>&\d/g, "").replace(/\d?>\s*(\/dev\/null|\$null|NUL)\b/gi, "");
    if (withoutSafeRedirects.includes(">") || !READ_ONLY_COMMANDS.some((p) => p.test(segment))) {
      reasons.push(`${READ_ONLY_REASON} — 허용되지 않은 명령: ${segment.slice(0, 80)}`);
    }
  }
  return reasons;
}

function editedPaths(tool: string, args: Record<string, unknown>): string[] {
  const path = args.file_path ?? args.notebook_path;
  if (typeof path === "string") return [path];
  if (tool === "apply_patch") {
    const patch = String(args.command ?? args.patch ?? args.input ?? "");
    return [...patch.matchAll(/^\*\*\* (?:Add|Update|Delete) File: (.+)$/gm)].map((m) => m[1].trim());
  }
  return [];
}

/** String-based on purpose: hosts send Windows paths that node:path on Linux would not treat as absolute. */
function repoRelative(path: string, cwd: string): string {
  const file = path.replace(/\\/g, "/");
  const root = cwd.replace(/\\/g, "/").replace(/\/+$/, "");
  return file.toLowerCase().startsWith(`${root.toLowerCase()}/`) ? file.slice(root.length + 1) : file;
}

/** New files live in folders that may not exist yet; ask git from the nearest existing parent. */
function gitAt(dir: string, args: string[]): string {
  let existing = dir;
  while (!existsSync(existing) && dirname(existing) !== existing) existing = dirname(existing);
  // A command may name a file; git runs in its folder.
  if (existsSync(existing) && !statSync(existing).isDirectory()) existing = dirname(existing);
  try {
    return execFileSync("git", args, { cwd: existing, encoding: "utf8", windowsHide: true, stdio: ["ignore", "pipe", "ignore"] }).trim();
  } catch {
    return "";
  }
}

export const realGit: GitProbe = {
  branch: (dir) => gitAt(dir, ["branch", "--show-current"]),
  isMirror: (dir) =>
    READ_ONLY_BRANCHES.includes(gitAt(dir, ["branch", "--show-current"])) &&
    PROJECT_REMOTE.test(gitAt(dir, ["remote", "get-url", "origin"])),
};

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  // Fail closed: a hook that cannot read its input or crashes blocks the call instead of waving it through.
  let reasons: string[];
  try {
    reasons = review(JSON.parse(readFileSync(0, "utf8")) as HookInput, realGit);
  } catch (error) {
    reasons = [`정책 훅이 입력을 검사하지 못했습니다: ${(error as Error).message}`];
  }
  if (reasons.length > 0) {
    console.error(`project_O 정책 훅이 차단했습니다:\n${[...new Set(reasons)].map((r) => `  - ${r}`).join("\n")}\n규칙: AGENTS.md · docs/DESIGN.md §8. 우회하지 말고 사용자에게 보고하세요.`);
    process.exit(2);
  }
}
