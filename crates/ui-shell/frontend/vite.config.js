import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  // The desktop shell loads these files from disk, not over a network, and
  // the chart engine is most of the bundle and is on screen at startup, so a
  // large first chunk costs nothing worth code-splitting for.
  build: { target: "es2021", outDir: "dist", chunkSizeWarningLimit: 1024 },
});
