import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

// The dev server port is fixed because `src-tauri/tauri.conf.json` points `devUrl` at it.
// Keep the two in sync.
export default defineConfig({
  plugins: [react()],
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // Rust sources are rebuilt by Cargo, not Vite.
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    css: false,
  },
});
