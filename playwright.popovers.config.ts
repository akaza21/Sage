import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/ui",
  testMatch: "w5-popovers.visual.ts",
  workers: 1,
  timeout: 30_000,
  reporter: "line",
  outputDir: "output/playwright/popovers",
  use: { browserName: "chromium", colorScheme: "dark" },
});
