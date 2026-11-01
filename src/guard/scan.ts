#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

import { CODE_RULES, READ_ONLY_BRANCHES, findHits, isAllowedPath } from "./rules.ts";

/**
 * Static policy scan, run by git hooks and CI.
 *
 *   --all       every tracked file (CI)
 *   --staged    staged content, and no commits on test/main (pre-commit)
 *   --pre-push  refuses pushes that target test/main (pre-push; refs on stdin)
 */
const mode = process.argv[2] ?? "--all";

function git(args: string[]): string {
  return execFileSync("git", args, { encoding: "utf8", windowsHide: true });
}

function scanFiles(files: string[], read: (path: string) => string): string[] {
  const problems: string[] = [];
  for (const file of files) {
    if (isAllowedPath(file)) continue;
    let text: string;
    try {
      text = read(file);
    } catch {
      continue; // deleted in this commit
    }
    if (text.includes("\u0000")) continue; // binary
    for (const hit of findHits(text, CODE_RULES)) problems.push(`${file}:${hit.line} [${hit.rule}] ${hit.reason}`);
  }
  return problems;
}

function fail(title: string, lines: string[]): never {
  console.error(`차단됨 — ${title}`);
  for (const line of lines) console.error(`  ${line}`);
  console.error("규칙: AGENTS.md · docs/DESIGN.md §8");
  process.exit(1);
}

if (mode === "--pre-push") {
  const refs = readFileSync(0, "utf8").split("\n").filter(Boolean);
  const protectedRefs = READ_ONLY_BRANCHES.map((b) => `refs/heads/${b}`);
  const blocked = refs.filter((line) => protectedRefs.includes(line.split(" ")[2]));
  if (blocked.length > 0) fail("test·main에는 직접 push할 수 없습니다. GitHub PR 병합으로만 바뀝니다", blocked);
} else if (mode === "--staged") {
  const branch = git(["branch", "--show-current"]).trim();
  if (READ_ONLY_BRANCHES.includes(branch)) fail(`${branch} 브랜치에서는 커밋할 수 없습니다. 자기 개인 브랜치나 <개인 브랜치>-<주제> 브랜치에서 작업하고 PR로 올리세요`, []);
  const files = git(["diff", "--cached", "--name-only", "--diff-filter=ACMR"]).split("\n").filter(Boolean);
  const problems = scanFiles(files, (path) => git(["show", `:${path}`]));
  if (problems.length > 0) fail("프로바이더 정책에 어긋나는 코드가 있습니다", problems);
} else {
  const files = git(["ls-files"]).split("\n").filter(Boolean);
  const problems = scanFiles(files, (path) => readFileSync(path, "utf8"));
  if (problems.length > 0) fail("프로바이더 정책에 어긋나는 코드가 있습니다", problems);
  console.log(`정책 검사 통과: ${files.length}개 파일`);
}
