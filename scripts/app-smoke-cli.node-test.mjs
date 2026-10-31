import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { randomUUID } from "node:crypto";
import { createServer as createHttpServer } from "node:http";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { basename, resolve, dirname } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { killProcessTree, portAccepts } from "./lib/app-smoke-server.mjs";
import { processAlive } from "./lib/process-liveness.mjs";

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const smokeEntry = resolve(repositoryRoot, "scripts", "app-smoke.mjs");
const CHILD_TIMEOUT_MS = 240_000;
const STDOUT_TAIL_LIMIT = 8_000;
const WATCHDOG_GRACE_MS = 5_000;
const runRoot = resolve(repositoryRoot, "work", "app-smoke");
const RUN_DIRECTORY =
  /^test-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/u;

function removeRun(directory) {
  const target = resolve(directory);
  if (dirname(target) !== runRoot) return false;
  if (!RUN_DIRECTORY.test(basename(target))) return false;
  rmSync(target, { recursive: true, force: true });
  return true;
}

function reserveFreePort() {
  return new Promise((done, fail) => {
    const probe = createServer();
    probe.once("error", fail);
    probe.listen(0, "127.0.0.1", () => {
      const { port } = probe.address();
      probe.close(() => done(port));
    });
  });
}

function servePage(t, body) {
  const site = createHttpServer((request, response) => {
    response.setHeader("content-type", "text/html");
    response.end(`<!doctype html><title>fixture</title>${body}`);
  });
  return new Promise((done) => {
    site.listen(0, "127.0.0.1", () => {
      t.after(
        () =>
          new Promise((closed) => {
            site.closeAllConnections();
            site.close(closed);
          }),
      );
      done({ port: site.address().port });
    });
  });
}

function inertControls(labels) {
  return labels
    .map((label) => {
      if (["Agent", "Editor"].includes(label)) {
        return `<label><input type="radio" name="mode" style="position:absolute;width:1px;height:1px;opacity:0;pointer-events:none">${label}</label>`;
      }
      if (label === "Open file README.md") {
        return `<button role="treeitem" aria-label="${label}">README.md</button>`;
      }
      return `<button type="button">${label}</button>`;
    })
    .join("");
}

const requiredControls = [
  "Open file README.md",
  "Search",
  "Search files",
  "Open settings",
  "Agent",
  "New chat",
];

function serveUnready(t) {
  const site = createHttpServer((request, response) => {
    response.statusCode = 503;
    response.end();
  });
  return new Promise((done) => {
    site.listen(0, "127.0.0.1", () => {
      t.after(
        () =>
          new Promise((closed) => {
            site.closeAllConnections();
            site.close(closed);
          }),
      );
      done({ port: site.address().port });
    });
  });
}

function runSmoke(
  t,
  args,
  environment = {},
  { timeoutMs = CHILD_TIMEOUT_MS, entry = smokeEntry } = {},
) {
  const directory = resolve(runRoot, `test-${randomUUID()}`);
  t.after(() => removeRun(directory));
  return new Promise((done, fail) => {
    const child = spawn(
      process.execPath,
      [entry, `--out=${directory}`, ...args],
      {
        cwd: repositoryRoot,
        env: { ...process.env, ...environment },
        stdio: ["ignore", "pipe", "pipe"],
        windowsHide: true,
      },
    );
    let surrender;
    const watchdog = setTimeout(async () => {
      const reaped = await killProcessTree(child.pid);
      if (!reaped) child.kill("SIGKILL");
      surrender = setTimeout(() => {
        fail(
          new Error(
            `smoke run pid ${child.pid} survived the watchdog; stdout tail: ${stdout}`,
          ),
        );
      }, WATCHDOG_GRACE_MS);
    }, timeoutMs);
    t.after(() => {
      clearTimeout(watchdog);
      clearTimeout(surrender);
      if (child.exitCode === null && child.signalCode === null) {
        return killProcessTree(child.pid);
      }
      return undefined;
    });
    let stdout = "";
    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");
    child.stdout.on("data", (chunk) => {
      stdout = (stdout + chunk).slice(-STDOUT_TAIL_LIMIT);
    });
    child.stderr.resume();
    child.once("error", (error) => {
      clearTimeout(watchdog);
      clearTimeout(surrender);
      fail(error);
    });
    child.once("close", (code) => {
      clearTimeout(watchdog);
      clearTimeout(surrender);
      const file = resolve(directory, "summary.json");
      const summary = existsSync(file)
        ? JSON.parse(readFileSync(file, "utf8"))
        : undefined;
      done({ code, stdout, summary, directory });
    });
  });
}

test(
  "a smoke run whose browser cannot launch still reports failure and releases its dev server",
  { timeout: 180_000 },
  async (t) => {
    const port = await reserveFreePort();
    const browsers = mkdtempSync(resolve(tmpdir(), "sage-smoke-browsers-"));
    t.after(() => rmSync(browsers, { recursive: true, force: true }));
    const run = await runSmoke(t, [`--port=${port}`], {
      PLAYWRIGHT_BROWSERS_PATH: browsers,
    });

    assert.equal(run.code, 1);
    assert.equal(typeof run.summary.fatal, "string");
    assert.equal(run.summary.settled, true);
    assert.equal(typeof run.summary.devServerPid, "number");
    assert.equal(processAlive(run.summary.devServerPid), false);
    assert.equal(await portAccepts("127.0.0.1", port), false);
  },
);

test(
  "a smoke run that never reaches a required surface fails the verdict",
  { timeout: 180_000 },
  async (t) => {
    const site = await servePage(t, "<main></main>");
    const run = await runSmoke(t, [`--url=http://127.0.0.1:${site.port}/`]);

    assert.equal(run.summary.fatal, null);
    assert.ok(run.summary.requiredMisses.length > 0);
    assert.equal(run.code, 1);
  },
);

test(
  "a smoke run whose dev server never starts still reports and settles",
  { timeout: 180_000 },
  async (t) => {
    const blocked = await serveUnready(t);
    const run = await runSmoke(t, [`--port=${blocked.port}`]);

    assert.equal(typeof run.summary.fatal, "string");
    assert.equal(typeof run.summary.devServerPid, "number");
    assert.equal(processAlive(run.summary.devServerPid), false);
    assert.equal(run.code, 1);
  },
);

test(
  "a smoke run whose required control is inert fails the verdict",
  { timeout: 180_000 },
  async (t) => {
    const site = await servePage(
      t,
      inertControls(["Editor", ...requiredControls]),
    );
    const run = await runSmoke(t, [`--url=http://127.0.0.1:${site.port}/`]);

    assert.equal(run.summary.fatal, null);
    assert.deepEqual(
      run.summary.misses.filter((entry) => requiredControls.includes(entry)),
      [],
    );
    assert.deepEqual(
      run.summary.requiredMisses.map((entry) => entry.split(" -> ")[0]),
      requiredControls,
    );
    assert.equal(run.code, 1);
  },
);

test(
  "a smoke run whose Settings entry is inert fails the required surface verdict",
  { timeout: 180_000 },
  async (t) => {
    const site = await servePage(t, inertControls(["Open settings"]));
    const run = await runSmoke(t, [`--url=http://127.0.0.1:${site.port}/`]);

    assert.equal(run.summary.fatal, null);
    assert.ok(!run.summary.misses.includes("Open settings"));
    assert.ok(
      run.summary.requiredMisses.some((entry) =>
        entry.startsWith("Open settings -> "),
      ),
    );
    assert.equal(run.code, 1);
  },
);

test(
  "a required control that leaves an already-visible surface unchanged fails the verdict",
  { timeout: 180_000 },
  async (t) => {
    const site = await servePage(
      t,
      `${inertControls(["Editor", ...requiredControls])}<section aria-label="Chat pane Existing" class="agent-chat-pane--focused"><textarea aria-label="Message agent"></textarea></section>`,
    );
    const run = await runSmoke(t, [`--url=http://127.0.0.1:${site.port}/`]);

    assert.equal(run.summary.fatal, null);
    assert.ok(
      run.summary.requiredMisses.some((entry) =>
        entry.startsWith("New chat -> "),
      ),
    );
    assert.equal(run.code, 1);
  },
);

test(
  "a smoke run types in the focused Chat when two composers are visible",
  { timeout: 180_000 },
  async (t) => {
    const site = await servePage(
      t,
      `${inertControls(["Editor", ...requiredControls.filter((label) => label !== "New chat")])}
      <button onclick="document.getElementById('existing').className = ''; document.getElementById('old-composer').readOnly = true; document.getElementById('next').hidden = false">New chat</button>
      <section id="existing" aria-label="Chat pane Existing" class="agent-chat-pane--focused"><textarea id="old-composer" aria-label="Message agent"></textarea></section>
      <section id="next" aria-label="Chat pane New chat" class="agent-chat-pane--focused" hidden><textarea aria-label="Message agent"></textarea></section>`,
    );
    const run = await runSmoke(t, [`--url=http://127.0.0.1:${site.port}/`]);

    assert.equal(run.summary.fatal, null);
    assert.ok(!run.summary.misses.includes("composer"));
    assert.ok(
      !run.summary.requiredMisses.some((entry) => entry.startsWith("New chat")),
    );
    assert.ok(run.summary.shots.includes("22-agent-composer-typed"));
  },
);

test(
  "a page dense with matching controls still reaches a bounded verdict",
  { timeout: 180_000 },
  async (t) => {
    const decoys =
      '<textarea aria-label="Message agent" style="display:none"></textarea>'.repeat(
        3_000,
      );
    const site = await servePage(
      t,
      `${inertControls(["Editor", ...requiredControls])}${decoys}`,
    );
    const started = Date.now();
    const run = await runSmoke(t, [`--url=http://127.0.0.1:${site.port}/`]);

    assert.equal(run.summary.fatal, null);
    assert.ok(
      run.summary.requiredMisses.some((entry) =>
        entry.startsWith("New chat -> "),
      ),
    );
    assert.equal(run.code, 1);
    assert.ok(Date.now() - started < 150_000);
  },
);

test("run cleanup refuses targets outside the smoke output subtree", (t) => {
  const outside = mkdtempSync(resolve(tmpdir(), "sage-smoke-outside-"));
  t.after(() => rmSync(outside, { recursive: true, force: true }));

  assert.equal(removeRun(outside), false);
  assert.equal(existsSync(outside), true);
  assert.equal(removeRun(resolve(runRoot, "..", "..", "src")), false);
  assert.equal(removeRun(runRoot), false);
  assert.equal(existsSync(resolve(repositoryRoot, "src")), true);

  const owned = resolve(runRoot, `test-${randomUUID()}`);
  const nested = resolve(owned, "nested");
  mkdirSync(nested, { recursive: true });
  t.after(() => rmSync(owned, { recursive: true, force: true }));

  assert.equal(removeRun(nested), false);
  assert.equal(existsSync(nested), true);
  assert.equal(removeRun(resolve(runRoot, "evil")), false);
  assert.equal(removeRun(resolve(runRoot, "test-not-a-uuid")), false);
  assert.equal(removeRun(owned), true);
  assert.equal(existsSync(owned), false);
});

test(
  "a smoke run that overruns its watchdog is reaped within a bounded wait",
  { timeout: 120_000 },
  async (t) => {
    const site = await servePage(t, "<main></main>");
    const started = Date.now();
    const run = await runSmoke(
      t,
      [`--url=http://127.0.0.1:${site.port}/`],
      {},
      { timeoutMs: 500 },
    ).catch((error) => error);

    assert.ok(Date.now() - started < 60_000);
    assert.equal(run instanceof Error, false);
    assert.equal(existsSync(run.directory), true);
  },
);

test(
  "a smoke run whose leader exits while a descendant holds its pipes still rejects",
  { timeout: 120_000 },
  async (t) => {
    const scratch = mkdtempSync(resolve(tmpdir(), "sage-smoke-leaky-"));
    const leaky = resolve(scratch, "leaky.mjs");
    const pidFile = resolve(scratch, "grandchild.pid");
    writeFileSync(
      leaky,
      [
        'import { spawn } from "node:child_process";',
        'import { writeFileSync } from "node:fs";',
        'const held = spawn(process.execPath, ["-e", "setTimeout(() => {}, 30000)"], {',
        "  detached: true,",
        '  stdio: ["ignore", 1, "ignore"],',
        "});",
        `writeFileSync(${JSON.stringify(pidFile)}, String(held.pid));`,
        "held.unref();",
      ].join("\n"),
    );
    t.after(async () => {
      if (existsSync(pidFile)) {
        await killProcessTree(Number(readFileSync(pidFile, "utf8").trim()));
      }
      rmSync(scratch, { recursive: true, force: true });
    });

    const started = Date.now();
    const run = await runSmoke(
      t,
      [],
      {},
      {
        timeoutMs: 500,
        entry: leaky,
      },
    ).catch((error) => error);

    assert.equal(run instanceof Error, true);
    assert.match(run.message, /^smoke run pid \d+ survived the watchdog/u);
    assert.ok(Date.now() - started < 30_000);
  },
);

test("killing a smoke run reaps the whole owned process tree", async () => {
  const parent = spawn(
    process.execPath,
    [
      "-e",
      "const { spawn } = require('node:child_process');" +
        "const child = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { stdio: 'ignore' });" +
        "console.log(child.pid);" +
        "setInterval(() => {}, 1000);",
    ],
    { stdio: ["ignore", "pipe", "ignore"], windowsHide: true },
  );
  const grandchild = await new Promise((done) => {
    parent.stdout.setEncoding("utf8");
    parent.stdout.once("data", (chunk) => done(Number(chunk.trim())));
  });

  assert.equal(processAlive(grandchild), true);
  await killProcessTree(parent.pid);
  await new Promise((done) => setTimeout(done, 1_000));

  assert.equal(processAlive(parent.pid), false);
  assert.equal(processAlive(grandchild), false);
});

test(
  "a self-hosted smoke run reaches every required surface and leaves no dev server behind",
  { timeout: 300_000 },
  async (t) => {
    const port = await reserveFreePort();
    const run = await runSmoke(t, [`--port=${port}`, "--strict"]);

    assert.deepEqual(run.summary.misses, []);
    assert.deepEqual(run.summary.requiredMisses, []);
    assert.deepEqual(run.summary.shots, [
      "10-editor-welcome",
      "11-editor-file-open",
      "12-search",
      "12-quick-open",
      "13-settings",
      "20-agent-panel",
      "21-agent-new-chat",
      "22-agent-composer-typed",
    ]);
    assert.equal(run.summary.settled, true);
    assert.equal(processAlive(run.summary.devServerPid), false);
    assert.equal(await portAccepts("127.0.0.1", port), false);
    assert.equal(run.code, 0);
  },
);
