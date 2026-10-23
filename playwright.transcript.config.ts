import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/ui",
  testMatch: ["activity-parity.visual.ts", "rich-answers.visual.ts"],
  workers: 1,
  timeout: 30_000,
  reporter: "line",
  outputDir: "output/playwright-transcript",
  use: { browserName: "chromium", colorScheme: "dark", locale: "en-US" },
});
