import assert from "node:assert/strict";
import { test } from "node:test";

import type { Plan } from "../src/core/plan.ts";
import { StreamReader, cardPrompt, launchSpec, unwrapShell } from "../src/core/providers.ts";
import { BYPASS_RULES, findHits } from "../src/guard/rules.ts";

const plan: Plan = {
  title: "랜딩 개편",
  project: "C:/work/demo",
  cards: [
    { id: "code", provider: "codex", role: "코딩", task: "구현", model: "gpt-6.1-sol", effort: "xhigh" },
    { id: "plan", provider: "claude", role: "설계", task: "설계", effort: "high" },
    { id: "design", provider: "agy", role: "디자인", task: "시안", model: "gemini-3.8-flash-high" },
  ],
};

const team = { runId: "r1", runDir: "C:/o/runs/r1" };

test("launch specs pin model and effort and never add a bypass flag", () => {
  const [codex, claude, agy] = plan.cards.map((card) => launchSpec(card, plan, team));
  assert.deepEqual(codex.args.slice(0, 5), ["exec", "--json", "-s", "workspace-write", "-C"]);
  assert.ok(codex.args.includes("model_reasoning_effort=xhigh"));
  assert.ok(codex.stdin?.includes("구현"));
  assert.ok(claude.args.includes("--effort") && !claude.args.includes("--model"));
  assert.equal(agy.stdin, undefined);
  assert.ok(agy.args.at(-1)?.includes("시안"));
  for (const spec of [codex, claude, agy]) assert.deepEqual(findHits(spec.args.join(" "), BYPASS_RULES), []);
});

test("the only opening each CLI gets is the run's team channel", () => {
  const [codex, claude, agy] = plan.cards.map((card) => launchSpec(card, plan, team));
  assert.deepEqual(claude.args.slice(claude.args.indexOf("--allowedTools"), claude.args.indexOf("--allowedTools") + 2), ["--allowedTools", "Bash(o msg *)"]);
  assert.equal(codex.args[codex.args.indexOf("--add-dir") + 1], "C:/o/runs/r1");
  // agy matches whole commands only: two fixed ones, and its outbox lives in the run folder.
  assert.deepEqual(JSON.parse(agy.args[agy.args.indexOf("--settings") + 1]), { permissions: { allow: ["command(o msg read)", "command(o msg post)"] } });
  assert.equal(agy.args[agy.args.indexOf("--add-dir") + 1], "C:/o/runs/r1");
  assert.match(agy.args.at(-1) ?? "", /C:\/o\/runs\/r1\/outbox\/design\.txt/);
  assert.deepEqual(codex.env, { O_RUN_ID: "r1", O_CARD_ID: "code" });
});

test("a card needs no role, and a role never picks a model", () => {
  const free: Plan = { ...plan, cards: [{ id: "a", provider: "codex", task: "README 오타 고치기" }, plan.cards[1]] };
  const prompt = cardPrompt(free.cards[0], free, team);
  assert.match(prompt, /^\[O 작업카드\] 랜딩 개편\n/);
  assert.match(prompt, /- plan: 설계 \(claude\)/);
  assert.match(prompt, /o msg send --to/);
  assert.ok(!launchSpec(free.cards[0], free, team).args.includes("-m"));
});

// Lines below are trimmed from real headless runs (2026-11-01).
test("claude stream: session, text, tools, limit and result", () => {
  const r = new StreamReader("claude");
  assert.deepEqual(r.read('{"type":"system","subtype":"init","session_id":"s1"}'), [{ kind: "session", sessionId: "s1" }]);
  assert.deepEqual(r.read('{"type":"system","subtype":"hook_started"}'), []);
  assert.deepEqual(
    r.read('{"type":"assistant","message":{"content":[{"type":"text","text":"ok"},{"type":"tool_use","name":"Edit","input":{"file_path":"src/a.ts"}}]}}'),
    [{ kind: "message", text: "ok" }, { kind: "tool", name: "Edit", summary: "src/a.ts" }],
  );
  assert.deepEqual(r.read('{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","resetsAt":1793526000}}'), []);
  assert.deepEqual(r.read('{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","resetsAt":1793526000}}'), [
    { kind: "limit", until: new Date(1793526000 * 1000).toISOString() },
  ]);
  assert.deepEqual(r.read('{"type":"result","subtype":"success","is_error":false,"result":"ok"}'), [{ kind: "final", ok: true, text: "ok" }]);
});

test("codex stream: config warnings are not failures", () => {
  const r = new StreamReader("codex");
  assert.deepEqual(r.read('{"type":"thread.started","thread_id":"t1"}'), [{ kind: "session", sessionId: "t1" }]);
  assert.deepEqual(r.read('{"type":"item.completed","item":{"id":"item_0","type":"error","message":"Codex is ignoring 1 unrecognized configuration setting."}}'), []);
  assert.deepEqual(r.read('{"type":"item.completed","item":{"id":"item_4","type":"agent_message","text":"ok"}}'), [{ kind: "message", text: "ok" }]);
  assert.deepEqual(r.read('{"type":"turn.completed","usage":{"input_tokens":1}}'), [{ kind: "final", ok: true, text: "" }]);
  const failed = r.read('{"type":"turn.failed","error":{"message":"You have hit your usage limit."}}');
  assert.deepEqual(failed.map((s) => s.kind), ["limit", "final"]);
});

test("agy stream: deltas become one message", () => {
  const r = new StreamReader("agy");
  assert.deepEqual(r.read('{"event":"init","conversation_id":"c1","init":{}}'), [{ kind: "session", sessionId: "c1" }]);
  assert.deepEqual(r.read('{"event":"step_update","step_update":{"state":"DONE","step_type":"user_input"}}'), []);
  assert.deepEqual(r.read('{"event":"step_update","step_update":{"state":"ACTIVE","step_type":"agent_response","text_delta":"o"}}'), []);
  assert.deepEqual(r.read('{"event":"step_update","step_update":{"state":"DONE","step_type":"agent_response","text_delta":"k\\n"}}'), [
    { kind: "message", text: "ok" },
  ]);
  assert.deepEqual(r.read('{"event":"result","result":{"status":"SUCCESS","response":"ok\\n"}}'), [{ kind: "final", ok: true, text: "ok" }]);
  assert.deepEqual(r.read("not json"), []);
});

// From a real run where headless agy could not get approval for `dir` (2026-11-01).
test("agy: a permission denial that ended the turn is a failure, not SUCCESS", () => {
  const r = new StreamReader("agy");
  assert.deepEqual(
    r.read('{"event":"step_update","step_update":{"state":"ACTIVE","step_type":"tool","tool_name":"run_command","tool_info":{"name":"run_command","parameters":{"CommandLine":"dir"}}}}'),
    [{ kind: "tool", name: "run_command", summary: "dir" }],
  );
  const [final] = r.read('{"event":"result","result":{"status":"SUCCESS","response":"","num_turns":1,"denied_actions":[{"action":"command","display_name":"RunCommand"}]}}');
  assert.equal(final.kind, "final");
  assert.equal(final.kind === "final" && final.ok, false);
  assert.match(final.kind === "final" ? final.text : "", /RunCommand/);
});

test("codex shell commands are shown without their shell wrapper", () => {
  assert.equal(unwrapShell('"C:\\Program Files\\PowerShell\\7.6.4\\pwsh.exe" -Command "Get-Content -LiteralPath \'index.html\'"'), "Get-Content -LiteralPath 'index.html'");
  assert.equal(unwrapShell("/bin/bash -lc 'npm test'"), "npm test");
  assert.equal(unwrapShell("git status"), "git status");
  const [tool] = new StreamReader("codex").read('{"type":"item.completed","item":{"type":"command_execution","command":"\\"C:\\\\pwsh.exe\\" -Command \\"git diff\\""}}');
  assert.deepEqual(tool, { kind: "tool", name: "shell", summary: "git diff" });
});

test("card prompts warn about parallel siblings and headless shells", () => {
  const [, claude, agy] = plan.cards.map((card) => cardPrompt(card, plan, team));
  assert.match(agy, /- code: 코딩 \(codex gpt-6\.1-sol\)/);
  assert.match(agy, /"o msg read"와 "o msg post" 두 개만/);
  assert.match(claude, /셸 명령은 승인할 사람이 없어/);
  const solo: Plan = { ...plan, cards: [plan.cards[0]] };
  assert.doesNotMatch(cardPrompt(solo.cards[0], solo, team), /동시에 일합니다/);
});
