import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/ui",
  testMatch: "chat-chrome.geometry.spec.ts",
  workers: 1,
  fullyParallel: false,
  reporter: "line",
  outputDir: "output/playwright-chrome",
  use: {
    browserName: "chromium",
    colorScheme: "dark",
    contextOptions: { reducedMotion: "reduce" },
  },
});
