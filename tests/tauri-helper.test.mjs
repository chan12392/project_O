import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, join } from "node:path";
import { test } from "node:test";

test("local CLI preparation builds the requested profile and Tauri target", () => {
  const dir = mkdtempSync(join(tmpdir(), "o-cargo-probe-"));
  const source = join(dir, "cargo.rs");
  const executable = join(dir, process.platform === "win32" ? "cargo.exe" : "cargo");
  writeFileSync(source, 'use std::io::Write; fn main() { let mut f = std::fs::OpenOptions::new().create(true).append(true).open(std::env::var_os("O_TEST_LOG").unwrap()).unwrap(); writeln!(f, "{}", std::env::args().skip(1).collect::<Vec<_>>().join(" ")).unwrap(); }');
  assert.equal(spawnSync("rustc", [source, "-o", executable], { windowsHide: true }).status, 0);
  const log = join(dir, "calls");
  const env = { ...process.env, PATH: `${dir}${delimiter}${process.env.PATH}`, O_TEST_LOG: log, TAURI_ENV_TARGET_TRIPLE: "test-target", TAURI_ENV_DEBUG: "true" };
  delete env.CARGO_BUILD_TARGET;
  const result = spawnSync(process.execPath, ["scripts/prepare-local-cli.mjs"], { env, windowsHide: true });
  assert.equal(result.status, 0, result.stderr.toString());
  assert.deepEqual(readFileSync(log, "utf8").trim().split(/\r?\n/), ["build -p o-cli", "build -p o-cli --target test-target"]);
  const releaseLog = join(dir, "release-calls");
  delete env.TAURI_ENV_DEBUG;
  env.CARGO_BUILD_TARGET = "test-target";
  env.O_TEST_LOG = releaseLog;
  const release = spawnSync(process.execPath, ["scripts/prepare-local-cli.mjs"], { env, windowsHide: true });
  assert.equal(release.status, 0, release.stderr.toString());
  assert.equal(readFileSync(releaseLog, "utf8").trim(), "build -p o-cli --release");
});
