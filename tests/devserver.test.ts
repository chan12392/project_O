import assert from "node:assert/strict";
import type { IncomingMessage } from "node:http";
import { Readable } from "node:stream";
import { test } from "node:test";

import { body, failureText, leadStopArgs } from "../app/dev/liveRuns.ts";

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
