import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// The app reads runs through the repo's pure core (../src/core), shared with the O CLI.
export default defineConfig({
  plugins: [react()],
  server: { port: 1430, strictPort: true, fs: { allow: [".."] } },
});
