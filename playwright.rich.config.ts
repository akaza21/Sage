import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/ui",
  testMatch: "rich-answers.visual.ts",
  workers: 1,
  reporter: "line",
  outputDir: "output/playwright/rich-answers",
  use: { browserName: "chromium", colorScheme: "dark" },
});
