import { execFileSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const args = ["build", "-p", "o-cli"];
if (!process.argv.includes("--debug") && process.env.TAURI_ENV_DEBUG !== "true") args.push("--release");
const target = process.env.TAURI_ENV_TARGET_TRIPLE;
const cwd = target ? resolve(root, "app/src-tauri") : root;
execFileSync("cargo", args, { cwd, stdio: "inherit", windowsHide: true });
if (target && target !== process.env.CARGO_BUILD_TARGET) {
  execFileSync("cargo", [...args, "--target", target], { cwd, stdio: "inherit", windowsHide: true });
}
