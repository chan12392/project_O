import assert from "node:assert/strict";
import { test } from "node:test";

import { keyedRefresh } from "../src/core/keyedRefresh.ts";

/** A reader whose readings finish when the test says so. */
function reader() {
  const waiting: { key: string; finish: (value: string) => void; fail: (error: Error) => void }[] = [];
  const read = (key: string) => new Promise<string>((finish, fail) => waiting.push({ key, finish, fail }));
  return { waiting, refresh: keyedRefresh(read) };
}
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

test("a refresh for another profile is read itself, after the one in flight", async () => {
  const { waiting, refresh } = reader();
  const a = refresh("project-o");
  const again = refresh("project-o");
  const b = refresh("own");
  await settle();
  assert.deepEqual(waiting.map((w) => w.key), ["project-o"], "the same key shares one reading; the other waits");
  waiting[0].finish("A");
  assert.deepEqual(await Promise.all([a, again]), ["A", "A"]);
  await settle();
  assert.deepEqual(waiting.map((w) => w.key), ["project-o", "own"]);
  waiting[1].finish("B");
  assert.equal(await b, "B");
});

test("a failed reading does not stop the next profile's, and a later refresh reads again", async () => {
  const { waiting, refresh } = reader();
  const a = refresh("project-o");
  const b = refresh("own");
  await settle();
  waiting[0].fail(new Error("probe failed"));
  await assert.rejects(a, /probe failed/);
  await settle();
  waiting[1].finish("B");
  assert.equal(await b, "B");
  await settle();
  const later = refresh("own");
  await settle();
  assert.equal(waiting.length, 3);
  waiting[2].finish("B2");
  assert.equal(await later, "B2");
});
