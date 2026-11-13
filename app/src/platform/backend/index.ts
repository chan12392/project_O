/**
 * What the screens ask of the machine beyond reading runs: which agent CLIs are
 * installed and signed in, and the official install and sign-in steps. In the
 * desktop app (project_O.exe) these are Tauri commands (tauri.ts); under the dev
 * server they are /api routes (dev.ts, app/dev/liveRuns.ts); in the recorded demo
 * they are a fixed answer (demo.ts). Each implements `Backend` (types.ts).
 */
import { demoBackend } from "./demo.ts";
import { devBackend } from "./dev.ts";
import { tauriBackend } from "./tauri.ts";
import type { Backend } from "./types.ts";

export * from "./types.ts";

/** The desktop app when Tauri is present, the dev server's /api otherwise; `demo` forces the recorded preview. */
export function backend(demo = false): Backend {
  if (demo) return demoBackend;
  const tauri = window.__TAURI_INTERNALS__;
  return tauri ? tauriBackend(tauri.invoke) : devBackend;
}

/** `backend` for this page: the recorded preview only when the link asks for it (`?source=demo`). */
export const currentBackend = () => backend(new URLSearchParams(window.location.search).get("source") === "demo");
