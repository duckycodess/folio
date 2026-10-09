import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  // `tauri build` sets TAURI_ENV_*; the desktop build uses it to leave the
  // browser preview's practice replies out of the bundle.
  envPrefix: ["VITE_", "TAURI_ENV_"],
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  test: {
    // Playwright owns `*.spec.ts`; Vitest must never try to run a browser
    // journey, and the fake native core's contract test must always run.
    include: ["src/**/*.test.ts", "e2e/**/*.test.ts"],
    exclude: ["**/node_modules/**", "**/dist/**", "**/*.spec.ts"],
  },
});
