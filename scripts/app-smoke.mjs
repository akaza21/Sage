#!/usr/bin/env node
// Real-app smoke: render Sage in a headless, isolated browser and walk the
// main surfaces, capturing screenshots and console errors.
//
//   node scripts/app-smoke.mjs            start Vite on SAGE_UI_PORT (default 1431)
//   node scripts/app-smoke.mjs --url=URL  drive an already-running server
//   --strict                               fail on any console error, not only page errors
//
// Output: work/app-smoke/<timestamp>/*.png plus summary.json (gitignored).
// The browser is Playwright's bundled Chromium with a fresh context; no user
// profile is touched. Without Tauri the app runs on its browser fixture ports,
// so this proves rendering and wiring, not native behaviour.

import { mkdirSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

import { settleDevServer, startDevServer } from "./lib/app-smoke-server.mjs";
import { resolveUiPort } from "./lib/ui-test-port.mjs";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const require = createRequire(import.meta.url);
const { chromium } = require("playwright");

const args = process.argv.slice(2);
const flag = (name) =>
  args.find((a) => a.startsWith(`--${name}=`))?.split("=")[1];
const strict = args.includes("--strict");
const port = resolveUiPort({
  ...process.env,
  SAGE_UI_PORT: flag("port") ?? process.env.SAGE_UI_PORT,
});
let url = flag("url");

const stamp = new Date().toISOString().replace(/[:.]/g, "-");
const out = flag("out")
  ? resolve(repo, flag("out"))
  : resolve(repo, "work", "app-smoke", stamp);
mkdirSync(out, { recursive: true });

const COMPOSER = 'textarea[aria-label="Message agent"]';
const EDITOR_TAB = '[role="tab"][aria-selected="true"]';
const SENTINEL = "sage-smoke-sentinel";
const DOM_SCAN_LIMIT = 50;
const NODE_SCAN_LIMIT = 2_000;
const SEARCH_INPUT = '[aria-label="Search text"]';
const AGENT_RAIL = 'aside[aria-label="Projects and chats"]';

let server;
let devServerPid = null;

const errors = [];
const misses = [];
const requiredMisses = [];
const shots = [];
let browser;
let page;

async function shot(name) {
  await page.waitForTimeout(600);
  const file = resolve(out, `${name}.png`);
  await page.screenshot({ path: file });
  shots.push(name);
  console.log(`SHOT ${name}`);
}
async function click(label, required = false, role = "button") {
  try {
    const control = page.getByRole(role, { name: label, exact: true });
    if (role === "radio") {
      // The mode radios are keyboard-accessible; their styled labels own clicks.
      if (!(await control.isChecked({ timeout: 4000 }))) {
        await control.press("Space", { timeout: 4000 });
      }
      if (!(await control.isChecked({ timeout: 4000 }))) {
        throw new Error(`${label} did not become checked`);
      }
    } else await control.click({ timeout: 4000 });
    return true;
  } catch {
    misses.push(label);
    if (required) requiredMisses.push(label);
    console.log(`MISS ${role}: ${label}`);
    return false;
  }
}

async function visible(selector) {
  const locator = page.locator(selector).first();
  if (!(await locator.isVisible().catch(() => false))) return null;
  return await locator.elementHandle().catch(() => null);
}

async function reach(label, expectation, role = "button") {
  const previous = await visible(expectation);
  if (!(await click(label, true, role))) {
    await previous?.dispose();
    return false;
  }
  const reached = await page
    .waitForFunction(
      ({ selector, before, matchLimit, nodeLimit }) => {
        const walker = globalThis.document.createTreeWalker(
          globalThis.document.body,
          globalThis.NodeFilter.SHOW_ELEMENT,
        );
        let visited = 0;
        let matched = 0;
        let node = walker.nextNode();
        while (node && visited < nodeLimit && matched < matchLimit) {
          visited += 1;
          if (node.matches(selector)) {
            matched += 1;
            if (node.getClientRects().length > 0) return node !== before;
          }
          node = walker.nextNode();
        }
        return false;
      },
      {
        selector: expectation,
        before: previous,
        matchLimit: DOM_SCAN_LIMIT,
        nodeLimit: NODE_SCAN_LIMIT,
      },
      { timeout: 5000 },
    )
    .then(() => true)
    .catch(() => false);
  await previous?.dispose();
  if (!reached) {
    requiredMisses.push(`${label} -> ${expectation}`);
    console.log(`MISS state: ${label}`);
  }
  return reached;
}

async function reachComposer(label) {
  const before = await page
    .evaluate(
      ({ selector, matchLimit, nodeLimit }) => {
        const walker = globalThis.document.createTreeWalker(
          globalThis.document.body,
          globalThis.NodeFilter.SHOW_ELEMENT,
        );
        let visited = 0;
        let matched = 0;
        let shown = 0;
        let node = walker.nextNode();
        while (node && visited < nodeLimit && matched < matchLimit) {
          visited += 1;
          if (node.matches(selector)) {
            matched += 1;
            if (node.getClientRects().length > 0) shown += 1;
          }
          node = walker.nextNode();
        }
        return shown;
      },
      {
        selector: COMPOSER,
        matchLimit: DOM_SCAN_LIMIT,
        nodeLimit: NODE_SCAN_LIMIT,
      },
    )
    .catch(() => 0);
  const primed = await focusedComposer()
    .fill(SENTINEL, { timeout: 4000 })
    .then(() => true)
    .catch(() => false);
  if (!(await click(label, true))) return false;
  const reached = await page
    .waitForFunction(
      ({ selector, sentinel, opened, matchLimit, nodeLimit }) => {
        const walker = globalThis.document.createTreeWalker(
          globalThis.document.body,
          globalThis.NodeFilter.SHOW_ELEMENT,
        );
        let visited = 0;
        let matched = 0;
        let shown = 0;
        let held = false;
        let node = walker.nextNode();
        while (node && visited < nodeLimit && matched < matchLimit) {
          visited += 1;
          if (node.matches(selector)) {
            matched += 1;
            if (node.getClientRects().length > 0) {
              shown += 1;
              if (shown > opened) return true;
              const text = "value" in node ? node.value : node.textContent;
              if ((text ?? "").trim() === sentinel) held = true;
            }
          }
          node = walker.nextNode();
        }
        return shown > 0 && !held;
      },
      {
        selector: COMPOSER,
        sentinel: SENTINEL,
        opened: before,
        matchLimit: DOM_SCAN_LIMIT,
        nodeLimit: NODE_SCAN_LIMIT,
      },
      { timeout: 5000 },
    )
    .then(() => true)
    .catch(() => false);
  if (!reached) {
    requiredMisses.push(
      `${label} -> ${primed ? "new or reset" : "visible"} composer`,
    );
    console.log(`MISS state: ${label}`);
  }
  return reached;
}

function focusedComposer() {
  return page
    .getByRole("region", { name: /^Chat pane / })
    .and(page.locator(".agent-chat-pane--focused"))
    .getByRole("combobox", { name: "Message agent", exact: true });
}

let fatal = null;
let settled;
try {
  if (!url) {
    url = `http://127.0.0.1:${port}/`;
    server = await startDevServer(repo, url).catch((error) => {
      server = error.server;
      devServerPid = error.devServerPid ?? null;
      throw error;
    });
    devServerPid = server.child.pid ?? null;
  }
  browser = await chromium.launch({ headless: true });
  const ctx = await browser.newContext({
    viewport: { width: 1600, height: 1000 },
  });
  page = await ctx.newPage();
  page.on("console", (m) => {
    if (m.type() === "error") errors.push(m.text().slice(0, 200));
  });
  page.on("pageerror", (e) =>
    errors.push(`pageerror: ${String(e).slice(0, 200)}`),
  );
  await page.goto(url, { waitUntil: "networkidle", timeout: 60_000 });
  await page.waitForTimeout(1500);
  await click("Editor", true, "radio");
  await shot("10-editor-welcome");

  await reach("Open file README.md", EDITOR_TAB, "treeitem");
  await shot("11-editor-file-open");

  await reach("Search", SEARCH_INPUT);
  await shot("12-search");

  await reach("Search files", '[role="dialog"][aria-label="Quick Open"]');
  await shot("12-quick-open");
  await page
    .getByRole("combobox", { name: "Search files by name", exact: true })
    .press("Escape", { timeout: 4000 })
    .catch(() => misses.push("Dismiss Quick Open"));

  await reach("Open settings", 'section[aria-label="Settings"]');
  await shot("13-settings");

  await reach("Agent", AGENT_RAIL, "radio");
  await page.waitForTimeout(1200);
  await shot("20-agent-panel");

  await reachComposer("New chat");
  await shot("21-agent-new-chat");

  const composer = focusedComposer();
  try {
    await composer.click({ timeout: 4000 });
    await composer.fill("Show me the README and summarise it.", {
      timeout: 4000,
    });
    await shot("22-agent-composer-typed");
  } catch {
    misses.push("composer");
  }
} catch (error) {
  fatal = String(error).slice(0, 400);
  console.error(`FATAL ${fatal}`);
} finally {
  if (page) await page.close().catch(() => {});
  if (browser) await browser.close().catch(() => {});
  settled = await settleDevServer(server);
}

const summary = {
  url,
  devServerPid,
  shots,
  misses,
  requiredMisses,
  errors,
  fatal,
  strict,
  settled,
};
writeFileSync(resolve(out, "summary.json"), JSON.stringify(summary, null, 2));
console.log(
  `shots=${shots.length} misses=${misses.length} consoleErrors=${errors.length}`,
);
errors.slice(0, 8).forEach((e) => console.log(`  ${e}`));
console.log(`output: ${out}`);

const pageErrors = errors.filter((e) => e.startsWith("pageerror:"));
const failed =
  fatal !== null ||
  pageErrors.length > 0 ||
  !settled ||
  requiredMisses.length > 0 ||
  (strict && (errors.length > 0 || misses.length > 0));
process.exit(failed ? 1 : 0);
