import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";

/** How to start an official CLI without a shell: the program, and arguments that go before the CLI's own. */
export interface CliCommand {
  command: string;
  prefix: string[];
}

/**
 * Finds an official CLI on PATH. npm installs CLIs as `.cmd` shims, which would
 * pass every argument through cmd.exe; we read which script the shim runs and
 * start it with node instead, so no argument is ever parsed by a shell. A shim
 * we cannot read is refused rather than run through cmd.exe.
 */
export function resolveCli(bin: string): CliCommand | undefined {
  const found = which(bin);
  if (!found) return undefined;
  if (!/\.(cmd|bat)$/i.test(found)) return { command: found, prefix: [] };
  const script = /"%dp0%\\([^"%]+\.(?:js|mjs|cjs))"/i.exec(readFileSync(found, "utf8"))?.[1];
  const path = script && join(dirname(found), script);
  return path && existsSync(path) ? { command: process.execPath, prefix: [path] } : undefined;
}

function which(bin: string): string | undefined {
  const exts = process.platform === "win32" ? [".exe", ".cmd", ".bat"] : [""];
  const sep = process.platform === "win32" ? ";" : ":";
  for (const dir of (process.env.PATH ?? "").split(sep)) {
    if (!dir) continue;
    for (const ext of exts) {
      const candidate = join(dir, bin + ext);
      if (existsSync(candidate)) return candidate;
    }
  }
  return undefined;
}
