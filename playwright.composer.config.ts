import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/ui",
  testMatch: "composer-parity.visual.ts",
  fullyParallel: false,
  workers: 1,
  timeout: 30_000,
  reporter: "line",
  outputDir: "output/playwright/composer-parity",
  use: {
    browserName: "chromium",
    colorScheme: "dark",
    locale: "en-US",
    timezoneId: "Asia/Calcutta",
    trace: "off",
    screenshot: "off",
    video: "off",
  },
});
