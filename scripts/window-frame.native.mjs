/* global window, innerWidth, innerHeight, devicePixelRatio */
import { spawn, execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import assert from "node:assert/strict";
import { createHash, randomUUID } from "node:crypto";
import { createInterface } from "node:readline";
import { chromium } from "@playwright/test";

// Usage: SAGE_UI_PORT=<dedicated port> node scripts/window-frame.native.mjs <exe> <evidence-dir>
// The executable must be built in this worktree with custom-protocol,acp-fixture.
assert.equal(process.platform, "win32");
const port = Number(process.env.SAGE_UI_PORT);
assert.ok(
  Number.isInteger(port) && port > 1024 && port < 65536,
  "dedicated SAGE_UI_PORT required",
);
const cycles = Number(process.env.CYCLES ?? "1000");
assert.ok(
  Number.isSafeInteger(cycles) && cycles > 0,
  "positive CYCLES required",
);
const phase = path.basename(process.argv[3] ?? "native-window");
const exe = path.resolve(process.argv[2]);
const out = path.resolve(process.argv[3]);
const root = path.resolve("work/cp-native-proof");
const run = path.join(root, `window-${randomUUID()}`);
const probe = path.resolve("scripts/window-frame-probe.ps1");
const occupied = await fetch(`http://127.0.0.1:${port}/json/version`).then(
  () => true,
  () => false,
);
assert.equal(occupied, false, "dedicated debug port must be unused");
const profile = path.join(run, "profile");
const project = path.join(run, "project");
const scratch = path.join(run, "temp");
for (const dir of [out, profile, project, scratch])
  fs.mkdirSync(dir, { recursive: true });
fs.writeFileSync(
  path.join(profile, ".sage-disposable-profile-v1"),
  "sage-disposable-profile-v1\n",
);
const env = { ...process.env };
for (const key of Object.keys(env))
  if (/^(SAGE_|WEBVIEW2_)/.test(key)) delete env[key];
Object.assign(env, {
  TEMP: scratch,
  TMP: scratch,
  SAGE_UI_PORT: String(port),
  SAGE_DISPOSABLE_PROFILE_ROOT: profile,
  SAGE_PROJECT_ROOT: project,
  WEBVIEW2_USER_DATA_FOLDER: path.join(profile, "webview"),
  WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-address=127.0.0.1 --remote-debugging-port=${port}`,
});
const fixture = path.join(path.dirname(exe), "acp-adversarial-fixture.exe");
assert.ok(fs.existsSync(fixture), "sibling ACP fixture required");
{
  const bytes = fs.readFileSync(fixture);
  Object.assign(env, {
    SAGE_CODEX_FIXTURE_PATH: fixture,
    SAGE_CODEX_FIXTURE_SHA256: createHash("sha256").update(bytes).digest("hex"),
    SAGE_CODEX_FIXTURE_BYTES: String(bytes.length),
    SAGE_CODEX_FIXTURE_CASE: "codex-tools-grouped",
  });
}
const child = spawn(exe, [], {
  env,
  cwd: project,
  windowsHide: true,
  stdio: ["ignore", "pipe", "pipe"],
});
const stream = fs.createWriteStream(path.join(out, "app.log"));
child.stdout.pipe(stream);
child.stderr.pipe(stream);
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const win = (action, extra = []) =>
  JSON.parse(
    execFileSync(
      "powershell.exe",
      [
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        probe,
        "-ProcessId",
        String(child.pid),
        "-Action",
        action,
        ...extra,
      ],
      {
        encoding: "utf8",
        windowsHide: true,
        timeout: 180000,
        env: { ...process.env, TEMP: scratch, TMP: scratch },
      },
    ),
  );
const rendererSize = (native) => {
  const renderer = native.children.find(
    (child) => child.cls === "Chrome_RenderWidgetHostHWND" && child.visible,
  );
  return renderer ? { w: renderer.w, h: renderer.h } : null;
};
const followsClient = (native, viewport) =>
  viewport.w === native.clientW &&
  viewport.h === native.clientH &&
  rendererSize(native)?.w === native.clientW &&
  rendererSize(native)?.h === native.clientH;
const report = { phase, pid: child.pid, states: [], errors: [] };
let browser;
let race;
try {
  for (let i = 0; i < 120; i++) {
    if (child.exitCode !== null) throw Error(`Sage exited ${child.exitCode}`);
    try {
      if ((await fetch(`http://127.0.0.1:${port}/json/version`)).ok) break;
    } catch {
      // Endpoint can be unavailable during startup or after shutdown.
    }
    await delay(500);
  }
  win("owner", ["-Port", String(port)]);
  browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  let page;
  for (let i = 0; i < 60 && !page; i++) {
    page = browser
      .contexts()
      .flatMap((c) => c.pages())
      .find((p) => /tauri\.localhost|^tauri:/.test(p.url()));
    if (!page) await delay(500);
  }
  assert.ok(page, "main Sage WebView exists");
  await page.locator(".title-bar").waitFor();
  await delay(1000);
  const ensureSearchModal = async () => {
    // Native window actions may dismiss transient search UI. Establish the
    // modal independently before each caption action that exercises it.
    if (!(await page.locator("dialog:modal").count())) {
      await page
        .locator(".title-bar")
        .getByRole("button", { name: "Search chats", exact: true })
        .click();
    }
    await page.locator("dialog:modal").waitFor();
  };
  page.on("pageerror", (e) => report.errors.push(String(e)));
  const diagnostics = await page.context().newCDPSession(page);
  diagnostics.on("Log.entryAdded", ({ entry }) => {
    if (entry.source === "security" && entry.level === "error")
      report.errors.push(entry.text);
  });
  await diagnostics.send("Log.enable");
  const initialWindow = win("state");
  win("hover", [
    "-X",
    String(Math.round(initialWindow.clientX + initialWindow.clientW / 2)),
    "-Y",
    String(initialWindow.clientY + 200),
  ]);
  for (const state of ["restore", "max", "snap"]) {
    const native = win(state, [
      "-OutPng",
      path.join(out, `${state}-native.png`),
    ]);
    await delay(500);
    const viewport = await page.evaluate(() => ({
      w: innerWidth,
      h: innerHeight,
      dpr: devicePixelRatio,
    }));
    assert.equal(viewport.dpr, 1, "DPR 1 capture");
    report.states.push({ state, native, viewport });
    if (state === "snap") {
      assert.equal(
        native.frameL,
        native.workArea.x,
        "OS snap aligns with left work-area edge",
      );
      assert.equal(
        native.frameT,
        native.workArea.y,
        "OS snap aligns with top work-area edge",
      );
      assert.ok(
        Math.abs(native.frameW - native.workArea.w / 2) <= 1,
        "OS snap occupies half the work area",
      );
      assert.equal(
        native.frameH,
        native.workArea.h,
        "OS snap fills work area height",
      );
    }
    assert.equal(viewport.w, native.clientW);
    assert.equal(viewport.h, native.clientH);
    console.log(
      `${phase} ${state}: client=${native.clientW}x${native.clientH} viewport=${viewport.w}x${viewport.h}`,
    );
    win("min");
  }
  win("restore");
  report.browserTab = await page.evaluate(async () => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    const tabId = await invoke("browser_open", { url: null });
    await invoke("browser_set_bounds", {
      tabId,
      x: 900,
      y: 120,
      width: 661,
      height: 87,
    });
    await invoke("browser_hide", { tabId });
    return tabId;
  });
  {
    race = spawn(
      "powershell.exe",
      [
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        probe,
        "-ProcessId",
        String(child.pid),
        "-Action",
        "serve",
      ],
      {
        windowsHide: true,
        stdio: ["pipe", "pipe", "pipe"],
        env: { ...process.env, TEMP: scratch, TMP: scratch },
      },
    );
    const replies = createInterface({ input: race.stdout })[
      Symbol.asyncIterator
    ]();
    await replies.next();
    const command = async (name) => {
      race.stdin.write(name + "\n");
      const line = await replies.next();
      if (line.done) throw Error("native race probe exited");
      return JSON.parse(line.value);
    };
    report.captureRace = [];
    for (let i = 0; i < 64; i++) {
      await command("sliver");
      await page.waitForFunction(
        () => innerWidth === 661 && innerHeight === 87,
      );
      const screenshot = page.screenshot({
        fullPage: i % 2 === 0,
        scale: "css",
      });
      const heldMinimized = i >= 60;
      const restoring = (async () => {
        await delay([0, 5, 15, 30][i % 4]);
        if (heldMinimized) {
          await command("min");
          await screenshot;
          await command("restore-only");
        } else {
          await command("restore");
        }
      })();
      await Promise.all([screenshot, restoring]);
      await delay(200);
      const native = await command("state");
      const viewport = await page.evaluate(() => ({
        w: innerWidth,
        h: innerHeight,
      }));
      report.captureRace.push({ i, heldMinimized, native, viewport });
      assert.equal(
        native.iconic,
        false,
        "external restore leaves Sage visible",
      );
      if (!followsClient(native, viewport)) {
        await delay(5000);
        report.captureRace.push({
          i,
          settled: await command("state"),
          viewport: await page.evaluate(() => ({
            w: innerWidth,
            h: innerHeight,
          })),
        });
        const settled = report.captureRace.at(-1);
        if (!followsClient(settled.settled, settled.viewport)) {
          win("state", [
            "-OutPng",
            path.join(out, "capture-race-mismatch-native.png"),
          ]);
          break;
        }
      }
    }
    race.stdin.write("quit\n");
  }
  for (const row of report.captureRace ?? []) {
    if (!row.settled) continue;
    assert.deepEqual(
      row.viewport,
      { w: row.settled.clientW, h: row.settled.clientH },
      "WebView follows restored native client after capture completes",
    );
    assert.deepEqual(
      rendererSize(row.settled),
      { w: row.settled.clientW, h: row.settled.clientH },
      "native renderer follows the restored client after capture completes",
    );
  }
  win("max");
  await page.reload();
  await page.locator(".title-bar").waitFor();
  await delay(500);
  const reloaded = win("state");
  const reloadedViewport = await page.evaluate(() => ({
    w: innerWidth,
    h: innerHeight,
  }));
  report.reload = { native: reloaded, viewport: reloadedViewport };
  assert.deepEqual(
    reloadedViewport,
    { w: reloaded.clientW, h: reloaded.clientH },
    "recreated renderer follows the current maximized client",
  );
  report.controls = await page.locator("sage-window-controls").count();
  if (report.controls) {
    win("restore");
    report.borders = win("borders");
    for (const border of report.borders.hits)
      assert.equal(
        border.actual,
        border.expected,
        "restored native resize border",
      );
    const dragRegion = await page
      .locator(".title-bar")
      .getAttribute("data-tauri-drag-region");
    assert.equal(dragRegion, "deep");
    const point = await page.locator(".title-bar__center").evaluate((node) => {
      const rect = node.getBoundingClientRect();
      return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
    });
    const restored = win("state");
    const coords = [
      "-X",
      String(Math.round(restored.clientX + point.x)),
      "-Y",
      String(Math.round(restored.clientY + point.y)),
    ];
    const dragged = win("drag", coords);
    assert.ok(
      dragged.x !== restored.x || dragged.y !== restored.y,
      "native title bar drags the window",
    );
    win("restore");
    win("doubleclick", coords);
    await delay(400);
    assert.equal(win("state").zoomed, true, "double-click maximizes");
    report.uia = win("uia");
    assert.equal(
      report.uia.buttons.length,
      3,
      "three real accessible native caption buttons",
    );
    assert.ok(
      report.uia.buttons.every(
        (b) => b.role === "ControlType.Button" && b.invoke && b.enabled,
      ),
    );
    win("uia", ["-ControlName", "Restore window"]);
    assert.equal(
      win("state").zoomed,
      false,
      "native accessible caption restores",
    );
    const maximum = win("state").children.find(
      (button) => button.name === "Maximize window",
    );
    const maximumPoint = [
      "-X",
      String(Math.round(maximum.x + maximum.w / 2)),
      "-Y",
      String(Math.round(maximum.y + maximum.h / 2)),
    ];
    assert.equal(
      win("cancelclick", maximumPoint).zoomed,
      false,
      "releasing outside a caption button cancels activation",
    );
    assert.equal(
      win("returnclick", maximumPoint).zoomed,
      true,
      "returning to the pressed caption button activates it",
    );
    win("uia", ["-ControlName", "Restore window"]);
    const top = win("hit", [
      "-X",
      String(Math.round(maximum.x + maximum.w / 2)),
      "-Y",
      String(maximum.y + 1),
    ]);
    assert.equal(
      top.hit,
      -1,
      "caption passes its top resize edge to the native frame",
    );
    win("uia", ["-ControlName", "Minimize window"]);
    assert.equal(win("state").iconic, true, "native caption minimizes");
    win("restore");
    await page
      .locator(".title-bar")
      .getByRole("button", { name: "Search chats", exact: true })
      .click();
    await page.locator("dialog:modal").waitFor();
    await ensureSearchModal();
    win("uia", ["-ControlName", "Maximize window"]);
    assert.equal(
      win("state").zoomed,
      true,
      "native maximize works during document modal",
    );
    await ensureSearchModal();
    win("uia", ["-ControlName", "Restore window"]);
    assert.equal(
      win("state").zoomed,
      false,
      "native restore works during document modal",
    );
    await ensureSearchModal();
    win("uia", ["-ControlName", "Minimize window"]);
    assert.equal(
      win("state").iconic,
      true,
      "native minimize works during document modal",
    );
    win("restore");
    await ensureSearchModal();
    for (const [name, shot] of [
      ["Close window", "close-hover-native.png"],
      ["Maximize window", "snap-hover-native.png"],
    ]) {
      const button = win("state").children.find(
        (b) => b.cls === "Button" && b.name === name,
      );
      assert.ok(button, `native ${name} exists`);
      const hit = win("hit", [
        "-X",
        String(Math.round(button.x + button.w / 2)),
        "-Y",
        String(Math.round(button.y + button.h / 2)),
      ]);
      assert.equal(
        hit.hit,
        name === "Maximize window" ? 9 : 20,
        "native caption hit target",
      );
      const hovered = win("hover", [
        "-X",
        String(Math.round(button.x + button.w / 2)),
        "-Y",
        String(Math.round(button.y + button.h / 2)),
        "-OutPng",
        path.join(out, shot),
      ]);
      report[name] = hovered;
      if (name === "Close window") {
        const pixel = hovered.pixels.find((p) => p.name === name);
        assert.deepEqual(
          [pixel.r, pixel.g, pixel.b],
          [196, 43, 28],
          "native close hover paints Windows red during modal",
        );
      }
    }
    await page.keyboard.press("Escape");
    await page.locator("dialog:modal").waitFor({ state: "hidden" });
    const maximized = win("max");
    const restoreButton = maximized.children.find(
      (button) => button.name === "Restore window",
    );
    report.maximizedSnapHover = win("hover", [
      "-X",
      String(Math.round(restoreButton.x + restoreButton.w / 2)),
      "-Y",
      String(Math.round(restoreButton.y + restoreButton.h / 2)),
      "-OutPng",
      path.join(out, "maximized-snap-hover-native.png"),
    ]);
    win("restore");
  }
  report.stress = { checked: 0, transient: 0, failures: [] };
  // Each 120-cycle batch preserves the probe's 60-cycle transition pattern
  // while keeping subprocess duration bounded and exposing progress.
  while (report.stress.checked < cycles) {
    const count = Math.min(120, cycles - report.stress.checked);
    const batch = win("stress", ["-Cycles", String(count)]);
    report.stress.checked += batch.checked;
    report.stress.transient += batch.transient;
    report.stress.failures.push(...batch.failures);
    console.log(JSON.stringify({ stress: report.stress }));
    assert.equal(batch.failures.length, 0, "WebView follows restore/resize");
    assert.equal(
      batch.checked,
      count,
      "every requested cycle restores visibly",
    );
  }
  fs.writeFileSync(
    path.join(out, "report.json"),
    JSON.stringify(report, null, 2),
  );
  console.log(
    JSON.stringify({ controls: report.controls, stress: report.stress }),
  );

  {
    for (const { native } of report.states) {
      assert.ok(
        native.clientY - native.frameT <= 1,
        "no native caption above Sage's title bar",
      );
    }
    assert.equal(
      win("uia").buttons.length,
      3,
      "three accessible caption controls",
    );
  }
  assert.equal(
    report.stress.failures.length,
    0,
    "WebView follows restore/resize",
  );
  assert.equal(
    report.errors.length,
    0,
    "native workbench has no script or CSP errors",
  );
  if (report.controls) {
    await page
      .locator(".title-bar")
      .getByRole("button", { name: "Search chats", exact: true })
      .click();
    await page.locator("dialog:modal").waitFor();
    win("uia", ["-ControlName", "Close window"]);
    for (let i = 0; i < 40 && child.exitCode === null; i++) await delay(500);
    assert.notEqual(
      child.exitCode,
      null,
      "native close follows graceful app exit during modal",
    );
  }
} finally {
  fs.writeFileSync(
    path.join(out, "report.json"),
    JSON.stringify(report, null, 2),
  );
  if (child.exitCode === null) {
    try {
      win("min");
    } catch {
      // The window can disappear while a graceful close finishes.
    }
  }
  await browser?.close().catch(() => {});
  race?.kill();
  if (child.exitCode === null) {
    try {
      execFileSync("taskkill.exe", ["/PID", String(child.pid), "/T", "/F"], {
        windowsHide: true,
        stdio: "ignore",
      });
    } catch {
      // The process can finish between checking its exit status and taskkill.
    }
  }
  stream.end();
  assert.equal(
    path.dirname(run),
    root,
    "disposable profile remains within the worktree",
  );
  fs.rmSync(run, {
    recursive: true,
    force: true,
    maxRetries: 5,
    retryDelay: 200,
  });
}
