import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/ui",
  testMatch: "activity-parity.spec.ts",
  fullyParallel: false,
  workers: 1,
  timeout: 30_000,
  outputDir: "work/activity-geometry-results",
  use: {
    browserName: "chromium",
    colorScheme: "dark",
    reducedMotion: "reduce",
  },
});
