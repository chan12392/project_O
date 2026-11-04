import { execFileSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const args = ["build", "-p", "o-cli"];
if (!process.argv.includes("--debug") && process.env.TAURI_ENV_DEBUG !== "true") args.push("--release");
execFileSync("cargo", args, { cwd: root, stdio: "inherit", windowsHide: true });
const target = process.env.TAURI_ENV_TARGET_TRIPLE;
if (target && target !== process.env.CARGO_BUILD_TARGET) {
  execFileSync("cargo", [...args, "--target", target], { cwd: root, stdio: "inherit", windowsHide: true });
}
