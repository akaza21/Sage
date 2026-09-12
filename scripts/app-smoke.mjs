#!/usr/bin/env node
// Real-app smoke: render Sage in a headless, isolated browser and walk the
// main surfaces, capturing screenshots and console errors.
//
//   node scripts/app-smoke.mjs            start Vite itself on 127.0.0.1:1431
//   node scripts/app-smoke.mjs --url=URL  drive an already-running server
//   --strict                               fail on any console error, not only page errors
//
// Output: work/app-smoke/<timestamp>/*.png plus summary.json (gitignored).
// The browser is Playwright's bundled Chromium with a fresh context; no user
// profile is touched. Without Tauri the app runs on its browser fixture ports,
// so this proves rendering and wiring, not native behaviour.

import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const require = createRequire(import.meta.url);
const { chromium } = require("playwright");

const args = process.argv.slice(2);
const flag = (name) =>
  args.find((a) => a.startsWith(`--${name}=`))?.split("=")[1];
const strict = args.includes("--strict");
const port = Number(flag("port") ?? 1431);
let url = flag("url");

const stamp = new Date().toISOString().replace(/[:.]/g, "-");
const out = resolve(repo, "work", "app-smoke", stamp);
mkdirSync(out, { recursive: true });

let vite;
if (!url) {
  url = `http://127.0.0.1:${port}/`;
  vite = spawn(
    process.platform === "win32" ? "npx.cmd" : "npx",
    ["vite", "--host", "127.0.0.1", "--port", String(port), "--strictPort"],
    {
      cwd: repo,
      stdio: ["ignore", "pipe", "pipe"],
      shell: process.platform === "win32",
    },
  );
  await waitForServer(url, 60_000);
}

const errors = [];
const misses = [];
const shots = [];
const browser = await chromium.launch({ headless: true });
const ctx = await browser.newContext({
  viewport: { width: 1600, height: 1000 },
});
const page = await ctx.newPage();
page.on("console", (m) => {
  if (m.type() === "error") errors.push(m.text().slice(0, 200));
});
page.on("pageerror", (e) =>
  errors.push(`pageerror: ${String(e).slice(0, 200)}`),
);

async function shot(name) {
  await page.waitForTimeout(600);
  const file = resolve(out, `${name}.png`);
  await page.screenshot({ path: file });
  shots.push(name);
  console.log(`SHOT ${name}`);
}
async function click(label) {
  try {
    await page
      .getByRole("button", { name: label })
      .first()
      .click({ timeout: 4000 });
    return true;
  } catch {
    try {
      await page
        .getByText(label, { exact: false })
        .first()
        .click({ timeout: 3000 });
      return true;
    } catch {
      misses.push(label);
      console.log(`MISS click: ${label}`);
      return false;
    }
  }
}

let fatal = null;
try {
  await page.goto(url, { waitUntil: "networkidle", timeout: 60_000 });
  await page.waitForTimeout(1500);
  await shot("10-editor-welcome");

  await click("README.md");
  await shot("11-editor-file-open");

  await click("Search files");
  await shot("12-search");

  try {
    await page
      .locator('[aria-label*="Settings" i], button[title*="Settings" i]')
      .first()
      .click({ timeout: 4000 });
  } catch {
    misses.push("Settings gear");
  }
  await shot("13-settings");

  await click("Agent");
  await page.waitForTimeout(1200);
  await shot("20-agent-panel");

  await click("New chat");
  await shot("21-agent-new-chat");

  const composer = page.locator('textarea, [contenteditable="true"]').first();
  try {
    await composer.click({ timeout: 4000 });
    await composer.fill("Show me the README and summarise it.");
    await shot("22-agent-composer-typed");
  } catch {
    misses.push("composer");
  }
} catch (error) {
  fatal = String(error).slice(0, 400);
  console.error(`FATAL ${fatal}`);
}

await page.close().catch(() => {});
await browser.close().catch(() => {});
if (vite) vite.kill();

const summary = { url, shots, misses, errors, fatal, strict };
writeFileSync(resolve(out, "summary.json"), JSON.stringify(summary, null, 2));
console.log(
  `shots=${shots.length} misses=${misses.length} consoleErrors=${errors.length}`,
);
errors.slice(0, 8).forEach((e) => console.log(`  ${e}`));
console.log(`output: ${out}`);

const pageErrors = errors.filter((e) => e.startsWith("pageerror:"));
const failed =
  fatal !== null || pageErrors.length > 0 || (strict && errors.length > 0);
process.exit(failed ? 1 : 0);

async function waitForServer(target, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(target);
      if (response.ok) return;
    } catch {
      /* not up yet */
    }
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error(
    `dev server at ${target} did not start within ${timeoutMs}ms`,
  );
}
