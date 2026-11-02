// Builds the release `o` CLI and copies it to app/src-tauri/binaries/o-<target triple>[.exe],
// the sidecar that the installer config (scripts/installer/tauri.installer.json) bundles
// next to project_O.exe. Runs as the installer build's beforeBuildCommand (`npm run sidecar`)
// and from scripts/build-installer.ps1. Plain `cargo build`/`tauri dev` do not need it.
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const exe = process.platform === "win32" ? ".exe" : "";

execFileSync("cargo", ["build", "--release", "-p", "o-cli"], { cwd: root, stdio: "inherit", windowsHide: true });

const host = execFileSync("rustc", ["-vV"], { encoding: "utf8" }).match(/^host: (\S+)$/m)?.[1];
if (!host) throw new Error("rustc -vV did not report a host target triple");

const targetDir = process.env.CARGO_TARGET_DIR ? resolve(process.env.CARGO_TARGET_DIR) : join(root, "target");
const from = join(targetDir, "release", `o${exe}`);
const binaries = join(root, "app", "src-tauri", "binaries");
const to = join(binaries, `o-${host}${exe}`);
mkdirSync(binaries, { recursive: true });
copyFileSync(from, to);
console.log(`sidecar: ${from} -> ${to}`);
