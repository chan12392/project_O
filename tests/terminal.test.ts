import assert from "node:assert/strict";
import { test } from "node:test";

import { term } from "../app/dev/liveRuns.ts";
import { devBackend } from "../app/src/platform/backend/dev.ts";
import { leadItems, type LeadEvent } from "../src/core/lead.ts";
import {
  agyApply,
  b64ToBytes,
  bytesToB64,
  composerAgentChoices,
  nextOffset,
  planGuideText,
  plansFolder,
  sessionForCard,
  sessionForThread,
  TERM_DESKTOP_ONLY,
  TERM_WRITE_CHUNK,
  termWriteChunks,
  termWriteQueue,
  termWriteQueueEnd,
  termWriteQueueFor,
  type AgyState,
  type TermSession,
} from "../src/core/terminal.ts";

const session = (change: Partial<TermSession>): TermSession => ({
  id: "t-1",
  provider: "agy",
  project: "C:\\work\\app",
  purpose: "lead",
  state: "running",
  exitCode: null,
  cols: 80,
  rows: 24,
  startedAt: "2026-01-01T00:00:00.000Z",
  ...change,
});

test("sessionForThread picks the thread's live lead session, else its newest one", () => {
  const sessions = [
    session({ id: "t-other", threadId: "other" }),
    session({ id: "t-old", threadId: "th", state: "exited", startedAt: "2026-01-01T00:00:00.000Z" }),
    session({ id: "t-live", threadId: "th", startedAt: "2026-01-02T00:00:00.000Z" }),
    session({ id: "t-card", purpose: "card", threadId: "th", runId: "r1", cardId: "c1" }),
  ];
  assert.equal(sessionForThread(sessions, "th")?.id, "t-live");
  assert.equal(sessionForThread(sessions, "none"), undefined);
  assert.equal(sessionForThread(undefined, "th"), undefined);
  assert.equal(sessionForThread(sessions, undefined), undefined);
  // All of the thread's sessions exited: the newest (last in the list) still shows, read-only.
  assert.equal(sessionForThread([session({ id: "a", threadId: "th", state: "exited" }), session({ id: "b", threadId: "th", state: "exited" })], "th")?.id, "b");
});

test("sessionForThread shows the newest of two live sessions, whatever order the list is in", () => {
  const older = session({ id: "t-20260101-000000-aa", threadId: "th", startedAt: "2026-01-01T00:00:00.000Z" });
  const newer = session({ id: "t-20260101-000005-bb", threadId: "th", state: "awaiting-paste", startedAt: "2026-01-01T00:00:05.000Z" });
  assert.equal(sessionForThread([older, newer], "th")?.id, newer.id);
  assert.equal(sessionForThread([newer, older], "th")?.id, newer.id);
  // The older one is still there once the newer exits.
  assert.equal(sessionForThread([older, { ...newer, state: "exited" }], "th")?.id, older.id);
});

test("sessionForThread after close and reopen: closed sessions drop out, a new live one wins", () => {
  const exited = session({ id: "a", threadId: "th", state: "exited", exitCode: 0 });
  // termClose removes the session from `o term list`: nothing to reopen.
  assert.equal(sessionForThread([exited].filter((s) => s.id !== "a"), "th"), undefined);
  // After the person opens a new terminal the list has both; the live one wins.
  const reopened = session({ id: "b", threadId: "th", state: "running", startedAt: "2026-01-02T00:00:00.000Z" });
  assert.equal(sessionForThread([exited, reopened], "th")?.id, "b");
  // awaiting-paste counts as live too.
  const waiting = session({ id: "c", threadId: "th", state: "awaiting-paste" });
  assert.equal(sessionForThread([exited, waiting], "th")?.id, "c");
});

test("TermSession fields accept the nulls the Rust JSON sends", () => {
  // `o term list` prints absent ids as null, not a missing key.
  const rust = JSON.parse(
    '{"id":"t-20261110-120000-ab","provider":"agy","project":"C:\\\\work\\\\app","purpose":"lead","threadId":null,"runId":null,"cardId":null,"state":"exited","exitCode":0,"cols":80,"rows":24,"startedAt":"2026-11-10T12:00:00.000Z"}',
  ) as TermSession;
  assert.equal(sessionForThread([rust], "th"), undefined);
  assert.equal(sessionForCard([rust], "r1", "c1"), undefined);
  assert.equal(rust.exitCode, 0);
  assert.equal(rust.state, "exited");
});

test("sessionForCard matches run and card, not other cards", () => {
  const sessions = [
    session({ id: "c1", purpose: "card", runId: "r1", cardId: "a" }),
    session({ id: "c2", purpose: "card", runId: "r1", cardId: "b" }),
    session({ id: "lead", threadId: "th" }),
  ];
  assert.equal(sessionForCard(sessions, "r1", "b")?.id, "c2");
  assert.equal(sessionForCard(sessions, "r1", "zzz"), undefined);
  assert.equal(sessionForCard(undefined, "r1", "a"), undefined);
});

test("nextOffset advances and reports a stale position after dropped bytes", () => {
  assert.deepEqual(nextOffset(10, { offset: 20, start: 0 }), { offset: 20, stale: false });
  assert.deepEqual(nextOffset(10, { offset: 20, start: 5 }), { offset: 20, stale: false });
  assert.deepEqual(nextOffset(3, { offset: 20, start: 5 }), { offset: 20, stale: true });
});

test("base64 helpers round-trip bytes including non-ASCII", () => {
  const bytes = new Uint8Array([0, 1, 127, 128, 255, 104, 105]);
  assert.deepEqual(b64ToBytes(bytesToB64(bytes)), bytes);
  assert.equal(bytesToB64(b64ToBytes("aGk=")), "aGk=");
});

test("composerAgentChoices lists Antigravity as the terminal lead, not a chat lead", () => {
  const agents = [
    { id: "claude", installed: true, loggedIn: true },
    { id: "codex", installed: true, loggedIn: false },
    { id: "agy", installed: true, loggedIn: true },
  ];
  const choices = composerAgentChoices(agents);
  const agy = choices.find((c) => c.value === "agy");
  assert.equal(agy?.label, "Antigravity · 터미널");
  assert.equal(agy?.disabled, false);
  assert.ok(choices.find((c) => c.value === "claude")?.disabled === false);
  assert.ok(choices.find((c) => c.value === "codex")?.disabled === true);
  assert.ok(choices.find((c) => c.value === "devin")?.disabled === true);
  // Installed but logged out: the terminal is still offered (the person logs in inside it).
  const uninstalled = composerAgentChoices([{ id: "agy", installed: false, loggedIn: null }]).find((c) => c.value === "agy");
  assert.equal(uninstalled?.disabled, true);
  assert.match(uninstalled?.hint ?? "", /설치되지 않았습니다/);
});

test("agyApply requires the acknowledgement to enable workers", () => {
  const off: AgyState = { workers: false, acknowledgedAt: null, confirmPaste: true };
  assert.equal(typeof agyApply(off, { workers: true }, "2026-01-01T00:00:00.000Z"), "string");
  const on = agyApply(off, { workers: true, ack: true }, "2026-01-01T00:00:00.000Z");
  assert.deepEqual(on, { workers: true, acknowledgedAt: "2026-01-01T00:00:00.000Z", confirmPaste: true });
  // Turning off forgets the acknowledgement; enabling again asks again.
  const back = agyApply(on as AgyState, { workers: false }, "2026-01-02T00:00:00.000Z");
  assert.deepEqual(back, { workers: false, acknowledgedAt: null, confirmPaste: true });
  assert.equal(typeof agyApply(back as AgyState, { workers: true }, "2026-01-03T00:00:00.000Z"), "string");
  // confirmPaste changes alone never need the acknowledgement.
  assert.deepEqual(agyApply(off, { confirmPaste: false }, "t"), { workers: false, acknowledgedAt: null, confirmPaste: false });
});

test("plansFolder names the <home>-lead sibling folder", () => {
  assert.equal(plansFolder("C:\\Users\\me\\AppData\\Local\\project_O", "th-1"), "C:\\Users\\me\\AppData\\Local\\project_O-lead/th-1/plans/");
  assert.equal(plansFolder("/home/me/.local/state/project_O/", "th"), "/home/me/.local/state/project_O-lead/th/plans/");
});

test("planGuideText names the folder and the plan shape, in Korean", () => {
  const text = planGuideText("X-lead/th/plans/");
  assert.match(text, /X-lead\/th\/plans\//);
  assert.match(text, /"cards"/);
  assert.match(text, /provider/);
});

test("termWriteChunks splits over 32 KiB without breaking a character", () => {
  const enc = new TextEncoder();
  // Small input stays one piece; empty input stays nothing to send.
  assert.equal(termWriteChunks(enc.encode("hello")).length, 1);
  assert.equal(termWriteChunks(enc.encode("")).length, 0);
  // A 4-byte emoji straddling the 32 KiB boundary moves whole to the next piece.
  const head = "a".repeat(TERM_WRITE_CHUNK - 2);
  const text = head + "😀" + "b".repeat(100);
  const pieces = termWriteChunks(enc.encode(text));
  assert.equal(pieces.length, 2);
  assert.equal(pieces[0].length, TERM_WRITE_CHUNK - 2);
  for (const p of pieces) assert.ok(p.length <= TERM_WRITE_CHUNK);
  // Every piece is valid UTF-8 on its own and they reassemble to the input.
  const dec = new TextDecoder();
  assert.equal(pieces.map((p) => dec.decode(p)).join(""), text);
});

test("termWriteQueue drops input queued behind a failed write until reset", async () => {
  const seen: string[] = [];
  const gates: { resolve: () => void; reject: (e: Error) => void }[] = [];
  const queue = termWriteQueue((b64) => {
    seen.push(b64);
    if (b64 === "after reset") return Promise.resolve();
    return new Promise<void>((resolve, reject) => gates.push({ resolve, reject }));
  });
  // Three writes enqueued before the first settles, as a fast paste would.
  const a = queue.send("first");
  const b = queue.send("second");
  const c = queue.send("third");
  b.catch(() => {});
  c.catch(() => {});
  await new Promise((r) => setTimeout(r, 0));
  assert.deepEqual(seen, ["first"]); // the rest wait their turn
  gates[0].reject(new Error("down"));
  await assert.rejects(a, /down/);
  await assert.rejects(b, /down/);
  await assert.rejects(c, /down/);
  // The failed and already-queued input was never sent.
  assert.deepEqual(seen, ["first"]);
  await assert.rejects(queue.send("fourth"), /down/);
  assert.deepEqual(seen, ["first"]);
  // The person's retry starts a fresh queue; nothing is re-sent on its own.
  queue.reset();
  await queue.send("after reset");
  assert.deepEqual(seen, ["first", "after reset"]);
});

test("termWriteQueueFor shares one FIFO per session across consumers", async () => {
  const order: string[] = [];
  // A slow transport: each send finishes a tick later, so parallel calls could overtake without the FIFO.
  const queue1 = termWriteQueueFor("t-shared", (b64) => {
    order.push(b64);
    return new Promise<void>((resolve) => setTimeout(resolve, 0));
  });
  const queue2 = termWriteQueueFor("t-shared", () => Promise.reject(new Error("a second view must reuse the first transport")));
  // Two views enqueue alternately while the first write is in flight.
  const a = queue1.send("a1");
  const b = queue2.send("a2");
  const c = queue1.send("a3");
  await Promise.all([a, b, c]);
  assert.deepEqual(order, ["a1", "a2", "a3"]); // strict FIFO, no overtaking
  termWriteQueueEnd("t-shared");
});

test("a reset right after a failure does not resurrect input queued behind it", async () => {
  const seen: string[] = [];
  let failFirst: (e: Error) => void = () => {};
  const queue = termWriteQueue((b64) => {
    seen.push(b64);
    if (b64 === "first") return new Promise<void>((_, reject) => (failFirst = reject));
    return Promise.resolve();
  });
  const a = queue.send("first");
  const b = queue.send("second");
  const c = queue.send("third");
  await new Promise((r) => setTimeout(r, 0));
  failFirst(new Error("down"));
  // The person's handler runs as soon as the first send rejects, before the queue reaches the rest.
  await a.catch(() => queue.reset());
  const d = queue.send("after reset");
  await assert.rejects(b);
  await assert.rejects(c);
  await d;
  assert.deepEqual(seen, ["first", "after reset"]);
  await queue.send("next");
  assert.deepEqual(seen, ["first", "after reset", "next"]);
});

test("a stale retry from a second view cannot overtake a send in flight", async () => {
  const log: string[] = [];
  const gates = new Map<string, () => void>();
  const queue = termWriteQueueFor("t-two-views", (b64) => {
    log.push(`start:${b64}`);
    if (b64 === "fail") return Promise.reject(new Error("down"));
    return new Promise<void>((resolve) => gates.set(b64, () => { log.push(`end:${b64}`); resolve(); }));
  });
  await assert.rejects(queue.send("fail"), /down/);
  // View A retries and starts a slow send; view B then presses its own stale retry and types.
  queue.reset();
  const slow = queue.send("slow");
  await new Promise((r) => setTimeout(r, 0));
  queue.reset();
  const later = queue.send("later");
  await new Promise((r) => setTimeout(r, 0));
  assert.deepEqual(log, ["start:fail", "start:slow"]); // "later" waits behind "slow"
  gates.get("slow")!();
  await slow;
  await new Promise((r) => setTimeout(r, 0));
  gates.get("later")!();
  await later;
  assert.deepEqual(log, ["start:fail", "start:slow", "end:slow", "start:later", "end:later"]);
  // Ended while idle: retired at once; ended with a send in flight: retired when it drains.
  const held = queue.send("held");
  await new Promise((r) => setTimeout(r, 0));
  termWriteQueueEnd("t-two-views");
  assert.equal(termWriteQueueFor("t-two-views", () => Promise.resolve()), queue);
  gates.get("held")!();
  await held;
  await new Promise((r) => setTimeout(r, 0));
  assert.notEqual(termWriteQueueFor("t-two-views", () => Promise.resolve()), queue);
  termWriteQueueEnd("t-two-views");
});

test("the dev server no longer offers the human-only term POSTs", async () => {
  const req = { method: "POST", headers: {}, url: "/api/term/open" } as import("node:http").IncomingMessage;
  const res = {} as import("node:http").ServerResponse;
  const query = new URLSearchParams();
  for (const path of ["/term/open", "/term/write", "/term/paste", "/term/close"]) {
    // Unhandled by the route table: the request falls through to a 404.
    assert.equal(await term(req, res, path, query), false, path);
  }
  assert.equal(await term({ ...req, method: "DELETE" } as import("node:http").IncomingMessage, res, "/term/read", query), false);
});

test("the dev backend cannot drive terminals and says so", async () => {
  assert.equal(devBackend.canDriveTerminals, false);
  const sentence = new RegExp(TERM_DESKTOP_ONLY.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
  await assert.rejects(devBackend.agySet({ workers: false }), sentence);
  await assert.rejects(devBackend.termOpen({ provider: "agy", project: "C:\\work" }), sentence);
  await assert.rejects(devBackend.termWrite("t-1", "aGk="), sentence);
  await assert.rejects(devBackend.termPaste("t-1"), sentence);
  await assert.rejects(devBackend.termClose("t-1"), sentence);
});

test("leadItems keeps a plan's notices as information, not violations", () => {
  const items = leadItems([
    {
      t: "lead.plan",
      at: "2026-01-01T00:00:00.000Z",
      planId: "p1",
      plan: { title: "계획", cards: [{ id: "a", provider: "agy", task: "할 일" }] },
      ok: true,
      violations: [],
      notices: ["agy 카드는 터미널로 실행됩니다", ""],
    } as LeadEvent,
  ]);
  const plan = items[0];
  assert.equal(plan.kind, "plan");
  if (plan.kind === "plan") {
    assert.deepEqual(plan.notices, ["agy 카드는 터미널로 실행됩니다"]);
    assert.deepEqual(plan.violations, []);
    assert.equal(plan.ok, true);
  }
});
