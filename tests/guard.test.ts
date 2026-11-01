import assert from "node:assert/strict";
import { test } from "node:test";

import { review } from "../src/guard/hook.ts";
import { CODE_RULES, findHits, isAllowedPath } from "../src/guard/rules.ts";

const onBranch = (name: string) => () => name;
const bash = (command: string) => ({ tool_name: "Bash", tool_input: { command }, cwd: "C:/repo" });

test("code that touches credentials, backends or bypass flags is found", () => {
  const samples = [
    "readFileSync(join(home, '.claude', '.credentials.json'))",
    "env.CLAUDE_CODE_OAUTH_TOKEN = token",
    "const p = join(codexHome, 'auth.json')",
    "fetch('https://api.anthropic.com/v1/messages')",
    "fetch('https://chatgpt.com/backend-api/codex/responses')",
    "url = 'https://cloudcode-pa.googleapis.com/v1internal'",
    "args.push('--dangerously-skip-permissions')",
  ];
  for (const sample of samples) assert.ok(findHits(sample, CODE_RULES).length > 0, sample);
  assert.deepEqual(findHits("spawn('claude', ['-p', '--output-format', 'stream-json'])", CODE_RULES), []);
});

test("only the rule file, its tests and docs may spell the patterns", () => {
  assert.ok(isAllowedPath("src/guard/rules.ts"));
  assert.ok(isAllowedPath("docs\\DESIGN.md"));
  assert.ok(!isAllowedPath("src/core/providers.ts"));
});

test("hook blocks shell access to credential files", () => {
  assert.ok(review(bash("cat ~/.claude/.credentials.json"), onBranch("a/x")).length > 0);
  assert.ok(review(bash("codex exec --dangerously-bypass-approvals-and-sandbox hi"), onBranch("a/x")).length > 0);
  assert.deepEqual(review(bash("node src/cli/o.ts status"), onBranch("a/x")), []);
});

test("hook enforces the branch flow", () => {
  assert.ok(review(bash("git push origin main"), onBranch("a/x")).length > 0);
  assert.ok(review(bash("git push origin HEAD:main"), onBranch("a/x")).length > 0);
  assert.ok(review(bash("git commit -m wip"), onBranch("main")).length > 0);
  assert.ok(review(bash("git push --force origin test"), onBranch("test")).length > 0);
  assert.deepEqual(review(bash("git push origin younchan"), onBranch("younchan")), []);
  assert.deepEqual(review(bash("git commit -m wip && git push origin younchan-o-cli"), onBranch("younchan-o-cli")), []);
});

test("hook enforces branch names", () => {
  assert.ok(review(bash("git switch -c feature-x"), onBranch("younchan")).length > 0);
  assert.ok(review(bash("git checkout -b younchan/x"), onBranch("younchan")).length > 0);
  assert.ok(review(bash("git switch -c a/x"), onBranch("younchan")).length > 0);
  assert.deepEqual(review(bash("git switch -c yongwoo-account-vault"), onBranch("yongwoo")), []);
  assert.deepEqual(review(bash("git switch -c younchan"), onBranch("main")), []);
  assert.deepEqual(review(bash("git branch -d younchan-old"), onBranch("younchan")), []);
});

test("hook blocks edits that introduce forbidden code, outside allowed files", () => {
  const edit = (file_path: string, content: string) => ({ tool_name: "Write", tool_input: { file_path, content }, cwd: "C:/repo" });
  const bad = "const t = process.env.CLAUDE_CODE_OAUTH_TOKEN";
  assert.ok(review(edit("C:/repo/src/core/x.ts", bad), onBranch("a/x")).length > 0);
  assert.deepEqual(review(edit("C:/repo/src/guard/rules.ts", bad), onBranch("a/x")), []);
  assert.deepEqual(review(edit("C:/repo/src/core/x.ts", "export const ok = 1"), onBranch("a/x")), []);

  const patch = "*** Begin Patch\n*** Update File: src/core/x.ts\n+fetch('https://api.openai.com/v1/responses')\n*** End Patch";
  assert.ok(review({ tool_name: "apply_patch", tool_input: { command: patch }, cwd: "C:/repo" }, onBranch("a/x")).length > 0);
});
