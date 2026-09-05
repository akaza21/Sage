import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/ui",
  testMatch: ["**/*.visual.ts", "**/agent-turn-journeys.spec.ts"],
  snapshotPathTemplate:
    "{testDir}/workbench.visual.spec.ts-snapshots/{arg}-{projectName}-{platform}{ext}",
  fullyParallel: false,
  workers: 1,
  timeout: 30_000,
  expect: {
    timeout: 5_000,
    toHaveScreenshot: {
      animations: "disabled",
      caret: "hide",
      maxDiffPixelRatio: 0.002,
    },
  },
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [["line"], ["html", { open: "never" }]] : "line",
  outputDir: "output/playwright",
  use: {
    baseURL: "http://127.0.0.1:1431",
    browserName: "chromium",
    colorScheme: "dark",
    locale: "en-US",
    timezoneId: "Asia/Calcutta",
    trace: "off",
    screenshot: "only-on-failure",
    video: "off",
  },
  projects: [{ name: "chromium" }],
  webServer: {
    command: "npm run dev -- --host 127.0.0.1 --port 1431 --strictPort",
    url: "http://127.0.0.1:1431",
    reuseExistingServer: false,
    timeout: 120_000,
    stdout: "pipe",
    stderr: "pipe",
  },
});
