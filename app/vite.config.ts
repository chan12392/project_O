import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

import { liveRuns } from "./dev/liveRuns.ts";

// The app reads runs through the repo's pure core (../src/core), shared with the O CLI.
// In dev, /api serves this machine's O runs and usage until the desktop shell exists.
export default defineConfig({
  plugins: [react(), tailwindcss(), liveRuns()],
  server: { port: 1430, strictPort: true, fs: { allow: [".."] } },
});
