#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { CODE_RULES, findHits, isAllowedPath } from "./rules.ts";

/**
 * PreToolUse hook for Claude Code and Codex working in this repository.
 * Exit code 2 blocks the tool call and hands the reason back to the agent.
 *
 * It refuses: shell commands that touch provider credentials or disable a
 * CLI's permission checks, code edits that introduce those, commits or
 * pushes to main, and branch names outside the <personal>-<topic> rule.
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

export function review(input: HookInput, currentBranch: () => string): string[] {
  const tool = input.tool_name ?? "";
  const args = input.tool_input ?? {};
  const cwd = input.cwd ?? process.cwd();
  const reasons: string[] = [];

  if (tool === "Bash" || tool === "shell" || tool === "PowerShell") {
    const command = String(args.command ?? "");
    for (const hit of findHits(command, CODE_RULES)) reasons.push(hit.reason);
    reasons.push(...gitReasons(command, currentBranch));
    return reasons;
  }

  const paths = editedPaths(tool, args).map((p) => repoRelative(p, cwd));
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
      else if (!/\bpush\s+\S+\s+\S+/.test(segment) && safeBranch(currentBranch) === "main") reasons.push("main 브랜치에서 push할 수 없습니다");
    }
    if (/\b(commit|merge|cherry-pick|rebase)\b/.test(segment) && safeBranch(currentBranch) === "main") {
      reasons.push("main 브랜치에서는 커밋·병합하지 않습니다. 자기 개인 브랜치나 <개인 브랜치>-<주제> 브랜치로 옮기세요");
    }
  }
  for (const match of command.matchAll(CREATE_BRANCH)) {
    const name = match[1];
    if (!BRANCH_NAME.test(name)) reasons.push(`브랜치 이름 ${name}은 규칙(${PERSONAL_BRANCHES.join(", ")} 또는 <개인 브랜치>-<주제>)에 맞지 않습니다`);
  }
  return reasons;
}

function editedPaths(tool: string, args: Record<string, unknown>): string[] {
  if (typeof args.file_path === "string") return [args.file_path];
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

function safeBranch(currentBranch: () => string): string {
  try {
    return currentBranch();
  } catch {
    return "";
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  let input: HookInput = {};
  try {
    input = JSON.parse(readFileSync(0, "utf8"));
  } catch {
    process.exit(0);
  }
  const reasons = review(input, () =>
    execFileSync("git", ["branch", "--show-current"], { cwd: input.cwd, encoding: "utf8", windowsHide: true }).trim(),
  );
  if (reasons.length > 0) {
    console.error(`project_O 정책 훅이 차단했습니다:\n${[...new Set(reasons)].map((r) => `  - ${r}`).join("\n")}\n규칙: AGENTS.md · docs/DESIGN.md §8. 우회하지 말고 사용자에게 보고하세요.`);
    process.exit(2);
  }
}
