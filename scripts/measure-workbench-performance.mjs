/* global window, document, requestAnimationFrame */
import { chromium } from "@playwright/test";
import { build, preview } from "vite";
import { writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

// Build the existing browser fixtures with production React and use a real
// clock. Native IPC and WebView2 performance require a separate native lane.
const root = path.resolve(process.argv[2] ?? ".");
const laneRoot = fileURLToPath(new URL("../", import.meta.url));
const relativeRoot = path.relative(laneRoot, root);
if (relativeRoot.startsWith("..") || path.isAbsolute(relativeRoot)) {
  throw new Error("Performance builds must stay inside this lane");
}
const output = path.resolve(process.argv[3] ?? "work/performance.json");
const width = Number(process.env.SAGE_PERF_WIDTH ?? "1000");
const port = Number(process.env.SAGE_UI_PORT);
if (port !== 1477) throw new Error("Set SAGE_UI_PORT=1477 for this lane");
const outDir = path.join(root, "work/q02-perf-browser");
await build({
  root,
  mode: "performance-browser",
  build: {
    outDir,
    emptyOutDir: true,
    reportCompressedSize: false,
    rolldownOptions: {
      input: [
        "index.html",
        "tests/ui/fixtures/chat-performance.html",
        "tests/ui/fixtures/workbench-performance.html",
      ].map((file) => path.join(root, file)),
    },
  },
});
const server = await preview({
  root,
  mode: "performance-browser",
  build: { outDir },
  preview: { host: "127.0.0.1", port, strictPort: true },
});
let browser;
try {
  console.log("Server listening", root);
  browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({
    viewport: { width, height: 900 },
    locale: "en-US",
    timezoneId: "Asia/Calcutta",
  });
  context.setDefaultTimeout(30_000);
  await context.addInitScript(() => {
    localStorage.setItem(
      "sage.workbench.layout.v1",
      JSON.stringify({
        editorToolRailWidth: 264,
        agentChatSplitPercent: 50,
        windowMode: "agent",
      }),
    );
    window.perfCommits = 0;
    window.perfTasks = [];
    window.__REACT_DEVTOOLS_GLOBAL_HOOK__ = {
      supportsFiber: true,
      renderers: new Map(),
      inject(renderer) {
        this.renderers.set(1, renderer);
        return 1;
      },
      onCommitFiberRoot: () => {
        window.perfCommits += 1;
      },
      onCommitFiberUnmount: () => {},
    };
    new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) {
        if (window.perfTasks.length === 256) window.perfTasks.shift();
        window.perfTasks.push({
          start: entry.startTime,
          duration: entry.duration,
        });
      }
    }).observe({ type: "longtask", buffered: true });
  });
  const page = await context.newPage();
  const base = `http://127.0.0.1:${port}`;
  const cold = [];
  // Fresh contexts prevent browser cache and persisted layout reuse. Keep the
  // first server-cold sample separately from subsequent fresh-browser samples.
  for (let index = 0; index < 7; index += 1) {
    const coldContext = await browser.newContext();
    coldContext.setDefaultTimeout(30_000);
    await coldContext.addInitScript(() => {
      localStorage.setItem(
        "sage.workbench.layout.v1",
        JSON.stringify({
          editorToolRailWidth: 264,
          agentChatSplitPercent: 50,
          windowMode: "agent",
        }),
      );
      // Record the first painted Chat inside the page. Host-side selector
      // polling would otherwise add its own retry interval to cold latency.
      let remainingFrames = 600;
      const firstChat = () => {
        if (
          document.querySelector("form.composer")?.getBoundingClientRect()
            .height > 0
        ) {
          requestAnimationFrame(() =>
            requestAnimationFrame(() => {
              window.perfFirstChatMs = performance.now();
            }),
          );
        } else if (--remainingFrames > 0) {
          requestAnimationFrame(firstChat);
        }
      };
      requestAnimationFrame(firstChat);
    });
    const coldPage = await coldContext.newPage();
    coldPage.on("pageerror", (error) =>
      console.log("Cold page error", error.message),
    );
    await coldPage.goto(base);
    try {
      await coldPage.waitForFunction(() => window.perfFirstChatMs > 0);
    } catch (error) {
      console.log(await coldPage.locator("body").innerText());
      await coldPage.screenshot({
        path: path.join(path.dirname(output), "performance-failure.png"),
      });
      throw error;
    }
    cold.push(await coldPage.evaluate(() => window.perfFirstChatMs));
    console.log("Cold load", index, cold.at(-1));
    await coldContext.close();
  }
  await page.bringToFront();
  await page.goto(`${base}/tests/ui/fixtures/workbench-performance.html`);
  await page.waitForFunction(() => Boolean(window.installPerfScenario));
  await page.evaluate(() => window.installPerfScenario("agent-dual"));
  console.log("Scenario installed");
  await page.locator(".acp-turn").first().waitFor();
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(500);
  const visibleChatPanes = await page
    .locator(".agent-chat-pane:visible")
    .count();
  console.log("Visible Chat panes", visibleChatPanes);
  if (visibleChatPanes !== 1)
    throw new Error("Compare exactly one visible Chat pane");
  const switches = [];
  for (let index = 0; index < 12; index += 1) {
    await page
      .getByRole("radio", { name: "Editor", exact: true })
      .evaluate((input) => input.click());
    await page.locator("main.workbench--editor").waitFor();
    // Deterministic phase jitter prevents the 100 ms wait locking samples to
    // one particular point of the display's refresh interval.
    await page.waitForTimeout(100 + ((index * 7) % 17));
    switches.push(
      await page.evaluate(async () => {
        const start = performance.now();
        const agent = document.querySelector(
          'input[name="workbench-window-mode"][value="agent"]',
        );
        if (!agent) throw new Error("Agent mode control missing");
        agent.click();
        for (let frame = 0; frame < 120; frame += 1) {
          await new Promise(requestAnimationFrame);
          if (
            document
              .querySelector("main.workbench--agent .message-list")
              ?.getBoundingClientRect().height > 0
          )
            break;
          if (frame === 119) throw new Error("Agent did not become visible");
        }
        await new Promise(requestAnimationFrame);
        const end = performance.now();
        await new Promise((resolve) => setTimeout(resolve, 100));
        return {
          durationMs: end - start,
          longestTaskMs: Math.max(
            0,
            ...window.perfTasks
              .filter(
                (task) =>
                  task.start + task.duration > start && task.start <= end,
              )
              .map((task) => task.duration),
          ),
        };
      }),
    );
    console.log("Mode switch", index, switches.at(-1));
  }
  const chatSwitches = [];
  console.log("Mode switches complete");
  for (let index = 0; index < 12; index += 1) {
    const buttons = page.locator(".project-chat-row button[data-chat-id]");
    if ((await buttons.count()) < 2)
      throw new Error("Need two fixture Chat rows");
    const before = await page.evaluate(() => window.perfCommits);
    const target = buttons.nth(index % 2);
    const chatId = await target.getAttribute("data-chat-id");
    await target.click();
    await page.waitForFunction((id) => window.perfChatSettled(id), chatId);
    await page.locator(`.agent-chat-pane[data-chat-id="${chatId}"]`).waitFor();
    await page.evaluate(async () => {
      let previous = window.perfCommits;
      let stableFrames = 0;
      for (let frame = 0; frame < 120 && stableFrames < 4; frame += 1) {
        await new Promise(requestAnimationFrame);
        const current = window.perfCommits;
        stableFrames = current === previous ? stableFrames + 1 : 0;
        previous = current;
      }
      if (stableFrames < 4) throw new Error("Chat commits did not settle");
    });
    chatSwitches.push(
      await page.evaluate((initial) => window.perfCommits - initial, before),
    );
  }
  if (!chatSwitches.slice(2).some((commits) => commits > 0)) {
    throw new Error("React commit hook did not observe Chat switching");
  }
  await page.goto(`${base}/tests/ui/fixtures/chat-performance.html`);
  await page.waitForFunction(() => Boolean(window.chatPerf));
  const mount = await page.evaluate(() => window.chatPerf.mount("stress"));
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(500);
  const scroll = await page.evaluate(async () => {
    const scroller = document.querySelector(".message-list");
    const frames = [];
    let last = await new Promise(requestAnimationFrame);
    for (let frame = 0; frame < 180; frame += 1) {
      scroller.scrollTop =
        (scroller.scrollHeight - scroller.clientHeight) *
        (0.5 + 0.5 * Math.sin(frame / 20));
      const now = await new Promise(requestAnimationFrame);
      frames.push(now - last);
      last = now;
    }
    return frames;
  });
  const percentile = (values, fraction) =>
    [...values].sort((a, b) => a - b)[Math.ceil(values.length * fraction) - 1];
  const result = {
    root,
    measuredAt: new Date().toISOString(),
    browserVersion: browser.version(),
    viewportWidth: width,
    coldViewport: { width: 1280, height: 720 },
    visibleChatPanes,
    runtime: "Production browser fixture build; real clock; Chromium",
    coldLoadMs: cold,
    coldLoadMedianMs: percentile(cold.slice(1), 0.5),
    editorToAgent: switches,
    editorToAgentMedianMs: percentile(
      switches.slice(2).map((entry) => entry.durationMs),
      0.5,
    ),
    editorToAgentLongestTaskMs: Math.max(
      ...switches.slice(2).map((entry) => entry.longestTaskMs),
    ),
    chatSwitchCommits: chatSwitches,
    chatSwitchMedianCommits: percentile(chatSwitches.slice(2), 0.5),
    longChatMount: mount,
    scrollFrameMedianMs: percentile(scroll, 0.5),
    scrollFrameP95Ms: percentile(scroll, 0.95),
    scrollFrameMaxMs: Math.max(...scroll),
    scrollFramesMs: scroll,
  };
  await writeFile(output, `${JSON.stringify(result, null, 2)}\n`);
  console.log(JSON.stringify(result));
} finally {
  await browser?.close();
  await new Promise((resolve, reject) => {
    server.httpServer.close((error) => (error ? reject(error) : resolve()));
    server.httpServer.closeAllConnections();
  });
}
