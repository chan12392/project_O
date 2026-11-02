import assert from "node:assert/strict";
import { test } from "node:test";

import { newlyFinished } from "../app/src/shell/finishedCards.ts";
import type { CardView, RunView } from "../src/core/snapshot.ts";

const card = (id: string, endedAt?: string) => ({ id, label: id, state: endedAt ? "done" : "running", endedAt }) as CardView;
const run = (runId: string, cards: CardView[]) => ({ runId, title: runId, cards }) as RunView;
const ids = (news: { run: RunView; card: CardView }[]) => news.map(({ run, card }) => `${run.runId}/${card.id}`);

test("a card finishing in any O run is news, not only in the run on screen", () => {
  const seen = new Set<string>();
  const known = new Set<string>();
  // First look: cards that already finished are history.
  assert.deepEqual(ids(newlyFinished([run("a", [card("1", "t")]), run("b", [card("1")])], seen, known, "a")), []);
  // Run b (not on screen) finishes a card while the person watches run a.
  assert.deepEqual(ids(newlyFinished([run("a", [card("1", "t")]), run("b", [card("1", "t")])], seen, known, "a")), ["b/1"]);
  // Told once.
  assert.deepEqual(ids(newlyFinished([run("a", [card("1", "t")]), run("b", [card("1", "t")])], seen, known, "a")), []);
  // A run dispatched later is watched from its first card on.
  assert.deepEqual(ids(newlyFinished([run("c", [card("1"), card("2", "t")])], seen, known)), []);
  assert.deepEqual(ids(newlyFinished([run("c", [card("1", "t"), card("2", "t")])], seen, known)), ["c/1"]);
});

test("Claude Code sessions are news only while on screen", () => {
  const seen = new Set<string>();
  const known = new Set<string>();
  const session = (ended?: string) => run("session-x", [card("1", ended)]);
  newlyFinished([session()], seen, known);
  assert.deepEqual(ids(newlyFinished([session("t")], seen, known)), []);
  const watchedSeen = new Set<string>();
  const watchedKnown = new Set<string>();
  newlyFinished([session()], watchedSeen, watchedKnown, "session-x");
  assert.deepEqual(ids(newlyFinished([session("t")], watchedSeen, watchedKnown, "session-x")), ["session-x/1"]);
});
