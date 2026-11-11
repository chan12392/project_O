// Builds the release `o` CLI and copies it to app/src-tauri/binaries/o-<target triple>[.exe],
// the sidecar that the installer config (scripts/installer/tauri.installer.json) bundles
// next to project_O.exe. Runs as the installer build's beforeBuildCommand (`npm run sidecar`)
// and from scripts/build-installer.ps1. Local Tauri builds prepare the CLI separately.
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const host = execFileSync("rustc", ["-vV"], { encoding: "utf8" }).match(/^host: (\S+)$/m)?.[1];
if (!host) throw new Error("rustc -vV did not report a host target triple");
const target = process.env.TAURI_ENV_TARGET_TRIPLE || process.env.CARGO_BUILD_TARGET;
const triple = target || host;
const exe = triple.includes("-windows-") ? ".exe" : "";
const args = ["build", "--release", "-p", "o-cli"];
if (target) args.push("--target", target);
// The installer build takes exactly the dependency versions in Cargo.lock.
if (process.env.PROJECT_O_LOCKED) args.push("--locked");
execFileSync("cargo", args, { cwd: root, stdio: "inherit", windowsHide: true });

const targetDir = process.env.CARGO_TARGET_DIR ? resolve(root, process.env.CARGO_TARGET_DIR) : join(root, "target");
const from = join(targetDir, ...(target ? [target] : []), "release", `o${exe}`);
const binaries = join(root, "app", "src-tauri", "binaries");
const to = join(binaries, `o-${triple}${exe}`);
mkdirSync(binaries, { recursive: true });
copyFileSync(from, to);
console.log(`sidecar: ${from} -> ${to}`);
