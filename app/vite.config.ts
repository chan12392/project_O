import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

import { liveRuns } from "./dev/liveRuns.ts";

// The app reads runs through the repo's pure core (../src/core), shared with the O CLI.
// In the browser dev server, /api serves this machine's O runs, usage and agent CLIs
// through `o`; the desktop shell (src-tauri, project_O.exe) answers the same with Tauri commands.
// Port 1430; src-tauri/tauri.conf.json devUrl (and devCsp) must match.
export default defineConfig({
  plugins: [react(), tailwindcss(), liveRuns()],
  clearScreen: false,
  server: { port: 1430, strictPort: true, fs: { allow: [".."] } },
});
