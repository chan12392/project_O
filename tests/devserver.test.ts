import assert from "node:assert/strict";
import type { IncomingMessage } from "node:http";
import { Readable } from "node:stream";
import { mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";
import { test } from "node:test";

import { body, failureText, leadStopArgs, oBinary } from "../app/dev/liveRuns.ts";

test("the dev server selects the CLI for its Tauri target and profile", () => {
  const names = ["CARGO_TARGET_DIR", "CARGO_BUILD_TARGET", "TAURI_ENV_TARGET_TRIPLE", "TAURI_ENV_DEBUG"] as const;
  const saved = names.map((name) => process.env[name]);
  const dir = mkdtempSync(join(tmpdir(), "o-dev-cli-"));
  try {
    process.env.CARGO_TARGET_DIR = dir;
    delete process.env.CARGO_BUILD_TARGET;
    process.env.TAURI_ENV_TARGET_TRIPLE = "test-target";
    process.env.TAURI_ENV_DEBUG = "true";
    const binary = join(dir, "test-target", "debug", `o${process.platform === "win32" ? ".exe" : ""}`);
    mkdirSync(join(dir, "test-target", "debug"), { recursive: true });
    writeFileSync(binary, "");
    assert.equal(oBinary(), binary);
    process.env.CARGO_TARGET_DIR = relative(join(process.cwd(), "app", "src-tauri"), dir);
    assert.equal(oBinary(), binary);
  } finally {
    names.forEach((name, i) => saved[i] === undefined ? delete process.env[name] : process.env[name] = saved[i]);
  }
});

test("a failed `o` says why even when it wrote nothing to stderr", () => {
  // An old `o` without `lead` prints its usage to stdout and exits 1.
  const usage = "usage: o <command>\n  dispatch  …\n  runs      …\n  status    …";
  const text = failureText("", usage, "Command failed: o lead show --json");
  assert.match(text, /usage: o <command>|status/);
  assert.match(text, /Command failed/);
  assert.equal(failureText("  정책 위반  ", "ignored", "Command failed"), "정책 위반");
  assert.equal(failureText("", "", "Command failed: o x"), "Command failed: o x");
});

test("stopping the lead in the dev server needs the project; it never stops every project's lead", () => {
  assert.equal(leadStopArgs(""), undefined);
  assert.equal(leadStopArgs("relative/dir"), undefined);
  assert.deepEqual(leadStopArgs(process.cwd()), ["lead", "stop", "--project", process.cwd()]);
});

test("an attachment over the limit is refused without cutting the connection, so its 413 reaches the page", async () => {
  const chunk = Buffer.alloc(64 * 1024, 1);
  const req = Readable.from(Array.from({ length: 4 }, () => chunk)) as unknown as IncomingMessage & Readable;
  await assert.rejects(body(req, 100 * 1024), /10MB/);
  await new Promise((resolve) => req.once("end", resolve).once("close", resolve));
  assert.equal(req.destroyed && !req.readableEnded, false, "the request was destroyed before its end");
  // Under the limit, the body comes back whole.
  const small = Readable.from([Buffer.from("ab"), Buffer.from("c")]) as unknown as IncomingMessage;
  assert.equal((await body(small, 10)).toString(), "abc");
});
