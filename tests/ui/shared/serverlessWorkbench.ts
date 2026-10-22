import { test as base } from "@playwright/test";
import { build } from "vite";
import react from "@vitejs/plugin-react";
import { mkdtemp, readdir, rm } from "node:fs/promises";
import { resolve, join } from "node:path";
import { tmpdir } from "node:os";

export const test = base.extend<object, { workbenchBuild: string }>({
  workbenchBuild: [
    async ({ browserName: _browserName }, provide) => {
      const directory = await mkdtemp(join(tmpdir(), "sage-popovers-"));
      try {
        await build({
          configFile: false,
          plugins: [react()],
          logLevel: "error",
          build: {
            outDir: directory,
            target: "es2022",
            rolldownOptions: {
              input: resolve("tests/ui/fixtures/popoverWorkbench.ts"),
              output: { entryFileNames: "workbench.js" },
            },
          },
        });
        await provide(directory);
      } finally {
        await rm(directory, { recursive: true, force: true });
      }
    },
    { scope: "worker", timeout: 120_000 },
  ],
  page: async ({ page, workbenchBuild }, provide) => {
    const styles = (await readdir(join(workbenchBuild, "assets"))).filter(
      (name) => name.endsWith(".css"),
    );
    await page.route("https://sage.test/**", async (route) => {
      const path = new URL(route.request().url()).pathname;
      if (path === "/") {
        await route.fulfill({
          contentType: "text/html",
          body: `<!doctype html><html lang="en"><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width, initial-scale=1.0, viewport-fit=cover">${styles.map((name) => `<link rel="stylesheet" href="/assets/${name}">`).join("")}</head><body><div id="root"></div><script type="module" src="/workbench.js"></script></body></html>`,
        });
      } else {
        const file = resolve(workbenchBuild, `.${path}`);
        if (
          !file.startsWith(workbenchBuild + "/") &&
          !file.startsWith(workbenchBuild + "\\")
        ) {
          await route.abort();
          return;
        }
        await route.fulfill({ path: file });
      }
    });
    await page.addInitScript(() =>
      localStorage.setItem(
        "sage.workbench.layout.v1",
        JSON.stringify({
          windowMode: "agent",
          editorToolRailWidth: 264,
          agentChatSplitPercent: 50,
        }),
      ),
    );
    await page.goto("https://sage.test/");
    await page.locator("main.workbench").waitFor();
    await provide(page);
  },
});
