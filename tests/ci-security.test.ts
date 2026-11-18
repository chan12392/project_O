import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { test } from "node:test";

test("the base branch policy binds personal branches to the PR author", () => {
  const workflow = readFileSync(new URL("../.github/workflows/branch-flow.yml", import.meta.url), "utf8");
  const script = workflow.split(/        run: \|\r?\n/)[1]!.replace(/^ {10}/gm, "");
  const member = /eunmi \| eunmi-\*\) echo "([^"]+)"/.exec(script)![1]!;
  const bash = process.platform === "win32" ? join(process.env.ProgramFiles!, "Git/bin/bash.exe") : "/bin/bash";
  const cases = [
    { head: "eunmi-fix", author: member, base: "test_2", files: "src/example.rs", ok: true },
    { head: "younchan-fix", author: member, base: "test_2", files: "src/example.rs", ok: false },
    { head: "eunmi-fix", author: member, base: "test_2", files: ".github/workflows/guard.yml", ok: false },
    { head: "eunmi-fix", author: "policy-owner", base: "test_2", files: ".github/workflows/guard.yml", ok: true },
    { head: "main", author: member, base: "younchan", files: "src/example.rs", ok: false },
    { head: "main", author: member, base: "eunmi", files: "src/example.rs", ok: true },
    { head: "yongwoo-fix", author: "unassigned-member", base: "test_2", files: "src/example.rs", ok: false },
    { head: "eunmi-fix", author: member, base: "test_2", files: "src/example.rs", ok: false, fork: true },
  ];
  for (const fixture of cases) {
    const result = spawnSync(bash, ["--noprofile", "--norc", "-eo", "pipefail", "-c", `gh() { printf '%s\\n' "$FIXTURE_FILES"; }\n${script}`], {
      windowsHide: true, encoding: "utf8", timeout: 10_000,
      env: { PATH: process.env.PATH, SystemRoot: process.env.SystemRoot, HEAD: fixture.head, BASE: fixture.base,
        AUTHOR: fixture.author, OWNER: "policy-owner", PERSONAL: "younchan yongwoo eunmi yeeun", NUMBER: "1",
        HEAD_REPOSITORY: fixture.fork ? "external/fork" : "team/project", BASE_REPOSITORY: "team/project", FIXTURE_FILES: fixture.files },
    });
    assert.ifError(result.error);
    assert.equal(result.status === 0, fixture.ok, `${fixture.head} -> ${fixture.base}: ${result.stdout}${result.stderr}`);
  }
});
