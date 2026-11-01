import assert from "node:assert/strict";
import { test } from "node:test";

import { review, type GitProbe } from "../src/guard/hook.ts";
import { CODE_RULES, PROJECT_REMOTE, findHits, isAllowedPath } from "../src/guard/rules.ts";

/** A project_O checkout on `name`; test/main checkouts are mirrors. */
const onBranch = (name: string): GitProbe => ({ branch: () => name, isMirror: () => name === "test" || name === "main" });
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
  assert.deepEqual(review(bash("git switch -c younchan"), onBranch("yongwoo")), []);
  assert.deepEqual(review(bash("git branch -d younchan-old"), onBranch("younchan")), []);
});

test("test and main checkouts are read-only for agents", () => {
  const mirror = (command: string) => ({ tool_name: "Bash", tool_input: { command }, cwd: "C:/ws/test" });
  for (const ok of ["git status", "git pull --ff-only", "git log --oneline -5", "npm test", "node --test tests/x.test.ts", "git diff 2>&1 | head -20", "ls > /dev/null"]) {
    assert.deepEqual(review(mirror(ok), onBranch("test")), [], ok);
  }
  const sneaky = ["ls & rm -rf src", "echo $(rm -rf src)", "rg --pre=calc x", "git grep -Ocalc x", "git diff --output=f", "grafly build"];
  for (const bad of ["git commit -m x", "git pull", "git checkout younchan", "npm install left-pad", "echo hi > notes.md", "rm -rf src", "git status && touch x", ...sneaky]) {
    assert.ok(review(mirror(bad), onBranch("test")).length > 0, bad);
  }
  assert.ok(review(mirror("sed -i s/a/b/ README.md"), onBranch("main")).length > 0);
  assert.deepEqual(review(mirror("sed -i s/a/b/ README.md"), onBranch("younchan")), []);

  // An edit is judged by the branch of the folder the file lives in, wherever the session runs.
  const inMirror = (dir: string) => dir.replace(/\\/g, "/").startsWith("C:/ws/test");
  const probe: GitProbe = { branch: (dir) => (inMirror(dir) ? "test" : "younchan"), isMirror: inMirror };
  const write = (file_path: string) => ({ tool_name: "Edit", tool_input: { file_path, new_string: "x" }, cwd: "C:/ws/younchan" });
  assert.ok(review(write("C:/ws/test/src/a.ts"), probe).length > 0);
  assert.ok(review(write("C:\\ws\\test\\new\\dir\\b.ts"), probe).length > 0);
  assert.deepEqual(review(write("C:/ws/younchan/src/a.ts"), probe), []);
  const notebook = { tool_name: "NotebookEdit", tool_input: { notebook_path: "C:/ws/test/n.ipynb", new_source: "x" }, cwd: "C:/ws/younchan" };
  assert.ok(review(notebook, probe).length > 0);

  // Naming the mirror from another folder is the same as working in it.
  const elsewhere = (command: string) => ({ tool_name: "Bash", tool_input: { command }, cwd: "C:/ws/younchan" });
  assert.ok(review(elsewhere("git -C C:/ws/test commit -m x"), probe).length > 0);
  assert.ok(review(elsewhere("rm C:/ws/test/src/a.ts"), probe).length > 0);
  assert.deepEqual(review(elsewhere("git -C C:/ws/test status"), probe), []);
});

test("sign-in files are off limits to read tools too", () => {
  assert.ok(review({ tool_name: "Read", tool_input: { file_path: "C:/Users/me/.codex/auth.json" } }, onBranch("younchan")).length > 0);
  assert.deepEqual(review({ tool_name: "Read", tool_input: { file_path: "C:/repo/README.md" } }, onBranch("younchan")), []);
});

test("another repository on main is not a mirror", () => {
  const otherRepoOnMain: GitProbe = { branch: () => "main", isMirror: () => false };
  const edit = { tool_name: "Edit", tool_input: { file_path: "C:/other/app/index.html", new_string: "x" }, cwd: "C:/ws/younchan" };
  assert.deepEqual(review(edit, otherRepoOnMain), []);
  assert.ok(PROJECT_REMOTE.test("https://github.com/chan12392/project_O.git"));
  assert.ok(PROJECT_REMOTE.test("git@github.com:chan12392/project_O"));
  assert.ok(!PROJECT_REMOTE.test("https://github.com/someone/project_Other.git"));
  assert.ok(!PROJECT_REMOTE.test(""));
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
