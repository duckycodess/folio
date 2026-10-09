import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
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
