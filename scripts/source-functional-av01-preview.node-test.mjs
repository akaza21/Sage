import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { connect } from "node:net";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";
import { EventEmitter } from "node:events";
import test from "node:test";

import {
  createAv01PreviewReceipt,
  parseAv01PreviewArguments,
  validateAv01FixtureProcessAbsence,
  validateAv01FixtureProcessCapture,
  validatePendingAv01PreviewReceipt,
} from "./source-functional-av01-preview-contract.mjs";
import {
  AV01_PREVIEW_CASES,
  createAv01PreviewFixture,
  readAv01PreviewFixtureIdentity,
} from "./source-functional-av01-preview-fixture.mjs";
import { parseAv01PreviewFinalizerArguments } from "./source-functional-av01-preview-finalizer.mjs";
import {
  AV01_PREVIEW_SNAPSHOT,
  AV01_PREVIEW_UI,
  decodeAv01FixtureProcessHelperOutput,
  previewManualOriginAction,
  runFixtureProcessHelper,
  runAv01PreviewJourney,
  validatePreviewSnapshot,
} from "./source-functional-av01-preview-probe.mjs";

const SHA = "a".repeat(64);
const GIT = "b".repeat(40);
const FIXTURE_NONCE = "c".repeat(64);
const FIXTURE_CAPTURE = Object.freeze({
  schemaVersion: 1,
  nonce: FIXTURE_NONCE,
  processId: 9123,
  executablePath: "C:\\fixture\\node.exe",
  creationDate: "2026-09-07T12:00:00.000Z",
  sageProcessId: 4100,
});

function root() {
  return mkdtempSync(path.join(tmpdir(), "sage-av01-preview-"));
}

function fixtureHelperOptions() {
  return {
    fixture: {
      path: "C:\\fixture",
      case: "starting-active",
      processHelper: "C:\\source\\source-functional-av01-preview-process.ps1",
    },
    sageProcess: {
      processId: 4100,
      creationDate: "2026-09-07T11:59:00.000Z",
    },
    executable: { path: "C:\\source\\sage.exe" },
    powershell: {
      path: "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe",
      bytes: 1,
      sha256: SHA,
    },
    sourceRoot: "C:\\source",
  };
}

function fixtureHelperChild({ stdout = "", stderr = "", code, emit = true }) {
  const child = new EventEmitter();
  child.stdout = new EventEmitter();
  child.stderr = new EventEmitter();
  child.killed = false;
  child.kill = () => {
    child.killed = true;
  };
  if (emit) {
    queueMicrotask(() => {
      if (stdout) child.stdout.emit("data", stdout);
      if (stderr) child.stderr.emit("data", stderr);
      child.emit("close", code);
    });
  }
  return child;
}

function argumentsFor(directory, overrides = {}) {
  const fixture = path.join(directory, "fixture-project");
  const values = {
    port: "9229",
    output: directory,
    "source-commit": GIT,
    "source-tree": GIT,
    executable: path.join(directory, "sage.exe"),
    "source-root": directory,
    "executable-bytes": "100",
    "executable-sha256": SHA,
    "node-bytes": "100",
    "node-sha256": SHA,
    "powershell-path": path.join(directory, "powershell.exe"),
    "powershell-bytes": "100",
    "powershell-sha256": SHA,
    "codex-path": path.join(directory, "codex.cmd"),
    "codex-bytes": "100",
    "codex-sha256": SHA,
    "profile-root": path.join(directory, "profile"),
    "profile-identity-artifact": path.join(
      directory,
      "acp04-profile-identity.json",
    ),
    "sage-stdout-log": path.join(directory, "sage.stdout.log"),
    "sage-stderr-log": path.join(directory, "sage.stderr.log"),
    "sage-process-id": "4100",
    "sage-process-created": "2026-09-07T12:00:00.000Z",
    "expected-origin": "http://127.0.0.1:1431/",
    "runtime-mode": "tauri-dev-split",
    "frontend-ready": "true",
    "receipt-name": "machine-receipt.pending.json",
    "deadline-ms": "90000",
    fixture,
    "fixture-identity": path.join(fixture, "fixture-identity.json"),
    "fixture-process-helper": path.join(
      directory,
      "source-functional-av01-preview-process.ps1",
    ),
    "preview-case": "starting-active",
    ...overrides,
  };
  return Object.entries(values).flatMap(([key, value]) => [`--${key}`, value]);
}

function waitForLine(child, timeout = 4_000) {
  return new Promise((resolve, reject) => {
    let output = "";
    let errors = "";
    const timer = setTimeout(
      () => reject(new Error("fixture output timeout")),
      timeout,
    );
    child.stdout.on("data", (chunk) => {
      output += chunk;
      const line = output.split(/\r?\n/u).find(Boolean);
      if (line) {
        clearTimeout(timer);
        resolve(line);
      }
    });
    child.stderr?.on("data", (chunk) => (errors += chunk));
    child.once("exit", (code) => {
      if (code !== 0 && !output) {
        clearTimeout(timer);
        reject(new Error(errors || `fixture exited ${code}`));
      }
    });
    child.once("error", reject);
  });
}

test("AV01 Preview parser accepts exactly the bounded three fixture cases", () => {
  const directory = root();
  try {
    for (const previewCase of AV01_PREVIEW_CASES) {
      const parsed = parseAv01PreviewArguments(
        argumentsFor(directory, { "preview-case": previewCase }),
      );
      assert.equal(parsed.fixture.case, previewCase);
    }
    assert.throws(
      () =>
        parseAv01PreviewArguments(
          argumentsFor(directory, { "preview-case": "synthetic" }),
        ),
      /av01-preview-arguments-invalid/,
    );
    assert.throws(
      () =>
        parseAv01PreviewArguments(argumentsFor(directory, { port: "+9229" })),
      /av01-preview-arguments-invalid/,
    );
    assert.throws(
      () =>
        parseAv01PreviewArguments(
          argumentsFor(directory, { "sage-process-id": "1.0" }),
        ),
      /av01-preview-arguments-invalid/,
    );
    assert.throws(
      () =>
        parseAv01PreviewArguments([
          ...argumentsFor(directory),
          "--mode",
          "bad",
        ]),
      /av01-preview-arguments-invalid/,
    );
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("AV01 Preview fixture hashes every package-script input and rejects mutation", () => {
  const directory = root();
  const fixture = path.join(directory, "fixture-project");
  try {
    const identity = createAv01PreviewFixture({
      fixture: (mkdirSync(fixture, { recursive: true }), fixture),
      previewCase: "starting-active",
      nonce: "1".repeat(64),
    });
    assert.equal(identity.files.length, 3);
    assert.equal(
      readAv01PreviewFixtureIdentity(fixture, "starting-active").nonce,
      "1".repeat(64),
    );
    writeFileSync(path.join(fixture, "preview-fixture.mjs"), "// altered\n", {
      flag: "a",
    });
    assert.throws(
      () => readAv01PreviewFixtureIdentity(fixture, "starting-active"),
      /av01-preview-fixture-identity-invalid/,
    );
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("AV01 process helper contract binds exact capture and disappearance identities", async () => {
  const helperSource = readFileSync(
    path.join(
      process.cwd(),
      "scripts",
      "source-functional-av01-preview-process.ps1",
    ),
    "utf8",
  );
  assert.equal(helperSource.includes("(if ("), false);
  assert.equal(helperSource.includes("Test-FootprintProcessIdentity"), false);
  assert.match(helperSource, /Get-FootprintRootIdentity/);
  assert.match(helperSource, /ConvertTo-FootprintProcessIdentity/);
  const capture = validateAv01FixtureProcessCapture(FIXTURE_CAPTURE, {
    nonce: FIXTURE_NONCE,
    sageProcessId: 4100,
  });
  const absence = { ...capture, absent: true };
  assert.deepEqual(
    validateAv01FixtureProcessAbsence(absence, capture),
    absence,
  );
  assert.throws(
    () =>
      validateAv01FixtureProcessCapture(
        { ...capture, nonce: "d".repeat(64) },
        { nonce: FIXTURE_NONCE, sageProcessId: 4100 },
      ),
    /av01-preview-fixture-process-invalid/,
  );
  assert.throws(
    () =>
      validateAv01FixtureProcessAbsence(
        { ...absence, creationDate: "2026-09-07T12:00:01.000Z" },
        capture,
      ),
    /av01-preview-fixture-process-absence-invalid/,
  );
  assert.throws(
    () =>
      decodeAv01FixtureProcessHelperOutput(
        JSON.stringify({ ...capture, injected: true }),
        "capture",
        { nonce: FIXTURE_NONCE, sageProcessId: 4100 },
      ),
    /av01-preview-fixture-process-json/,
  );
  assert.throws(
    () =>
      decodeAv01FixtureProcessHelperOutput("x".repeat(8_193), "capture", {
        nonce: FIXTURE_NONCE,
        sageProcessId: 4100,
      }),
    /av01-preview-fixture-process-json/,
  );

  const options = fixtureHelperOptions();
  const runtimeIdentity = { bytes: 1, sha256: SHA };
  const captureResult = await runFixtureProcessHelper(
    options,
    "capture",
    null,
    {
      platform: "win32",
      runtimeIdentity,
      readFixtureIdentity: () => ({ nonce: FIXTURE_NONCE }),
      spawn: () =>
        fixtureHelperChild({ stdout: JSON.stringify(capture), code: 0 }),
    },
  );
  assert.deepEqual(captureResult, capture);
  const absenceResult = await runFixtureProcessHelper(
    options,
    "absent",
    capture,
    {
      platform: "win32",
      runtimeIdentity,
      readFixtureIdentity: () => ({ nonce: FIXTURE_NONCE }),
      spawn: () =>
        fixtureHelperChild({ stdout: JSON.stringify(absence), code: 0 }),
    },
  );
  assert.deepEqual(absenceResult, absence);
  await assert.rejects(
    runFixtureProcessHelper(options, "capture", null, {
      platform: "win32",
      runtimeIdentity,
      readFixtureIdentity: () => ({ nonce: FIXTURE_NONCE }),
      spawn: () => fixtureHelperChild({ stderr: "native-failure", code: 1 }),
    }),
    /av01-preview-fixture-process-native-failure/,
  );
  await assert.rejects(
    runFixtureProcessHelper(options, "capture", null, {
      platform: "win32",
      runtimeIdentity,
      readFixtureIdentity: () => ({ nonce: FIXTURE_NONCE }),
      spawn: () => fixtureHelperChild({ stdout: "{", code: 0 }),
    }),
    /av01-preview-fixture-process-json/,
  );
  const waiting = fixtureHelperChild({ emit: false });
  await assert.rejects(
    runFixtureProcessHelper(options, "capture", null, {
      platform: "win32",
      runtimeIdentity,
      timeoutMs: 1,
      readFixtureIdentity: () => ({ nonce: FIXTURE_NONCE }),
      spawn: () => waiting,
    }),
    /av01-preview-fixture-process-timeout/,
  );
  assert.equal(waiting.killed, true);
});

test("AV01 pending receipt binds the exact captured identity to its absence proof", () => {
  const directory = root();
  const fixture = path.join(directory, "fixture-project");
  try {
    mkdirSync(fixture, { recursive: true });
    createAv01PreviewFixture({
      fixture,
      previewCase: "starting-active",
      nonce: FIXTURE_NONCE,
    });
    const helper = path.join(
      directory,
      "source-functional-av01-preview-process.ps1",
    );
    const runtime = path.join(directory, "powershell.exe");
    writeFileSync(helper, "# helper identity is source-bound\n");
    writeFileSync(runtime, "runtime\n");
    writeFileSync(
      path.join(directory, "av01-preview-observation.json"),
      "{}\n",
    );
    writeFileSync(path.join(directory, "av01-preview.png"), "png");
    const receipt = createAv01PreviewReceipt(
      {
        fixture: {
          path: fixture,
          case: "starting-active",
          processHelper: helper,
        },
        sageProcess: {
          processId: 4100,
          creationDate: "2026-09-07T11:59:00.000Z",
        },
        executable: { path: "C:\\source\\sage.exe", bytes: 1, sha256: SHA },
        powershell: {
          path: runtime,
          bytes: Buffer.byteLength("runtime\n"),
          sha256: createHash("sha256").update("runtime\n").digest("hex"),
        },
        node: { path: process.execPath, bytes: 1, sha256: SHA },
        codex: { path: "C:\\source\\codex.cmd", bytes: 1, sha256: SHA },
        source: { commit: GIT, tree: GIT },
        runtimeMode: "tauri-dev-split",
        frontendReady: true,
        port: 9229,
        expectedOrigin: "http://127.0.0.1:1431/",
        output: directory,
      },
      {
        startedAt: "2026-09-07T12:00:00.000Z",
        completedAt: "2026-09-07T12:01:00.000Z",
        modeA: {
          state: {
            phase: "complete",
            projectId: "project-a",
            chatId: "chat-a",
            sessionId: "session-a",
            runtimeEpoch: "1",
          },
        },
        preview: {
          case: "starting-active",
          finalPhase: "active",
          activeFrameUrl: "http://127.0.0.1:41234/",
          manualOrigin: null,
          fixtureProcess: FIXTURE_CAPTURE,
          fixtureRetirement: { ...FIXTURE_CAPTURE, absent: true },
        },
      },
      { id: "canonical-page" },
      ["av01-preview-observation.json", "av01-preview.png"],
      { connectionCount: 1, maxInFlight: 1, requestCount: 12 },
    );
    assert.equal(
      validatePendingAv01PreviewReceipt(receipt, directory).preview
        .fixtureProcess.processId,
      FIXTURE_CAPTURE.processId,
    );
    const mismatched = {
      ...receipt,
      preview: {
        ...receipt.preview,
        fixtureRetirement: {
          ...receipt.preview.fixtureRetirement,
          creationDate: "2026-09-07T12:00:01.000Z",
        },
      },
    };
    assert.throws(
      () => validatePendingAv01PreviewReceipt(mismatched, directory),
      /av01-preview-pending-invalid/,
    );
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("healthy fixture delays then serves its nonce marker; failed and exited cases do not fabricate it", async () => {
  const directory = root();
  const children = [];
  try {
    for (const previewCase of AV01_PREVIEW_CASES) {
      const fixture = path.join(directory, previewCase);
      mkdirSync(fixture, { recursive: true });
      const identity = createAv01PreviewFixture({
        fixture,
        previewCase,
        nonce: "2".repeat(64),
      });
      const child = spawn(process.execPath, ["preview-fixture.mjs"], {
        cwd: fixture,
        stdio: ["ignore", "pipe", "pipe"],
      });
      children.push(child);
      if (previewCase === "exited") {
        await new Promise((resolve) => child.once("exit", resolve));
        assert.equal(
          readFileSync(
            path.join(fixture, "av01-preview-startup.json"),
            "utf8",
          ).includes(identity.nonce),
          true,
        );
        continue;
      }
      const origin = await waitForLine(child);
      if (previewCase === "health-failed") {
        const parsed = new URL(origin);
        const bytes = await new Promise((resolve, reject) => {
          const socket = connect(Number(parsed.port), parsed.hostname);
          let result = "";
          socket.setEncoding("utf8");
          socket.on("data", (chunk) => (result += chunk));
          socket.once("close", () => resolve(result));
          socket.once("error", reject);
        });
        assert.equal(bytes, "AV01_NON_HTTP_HEALTH_FAILURE\r\n");
      } else {
        const response = await fetch(origin);
        assert.match(await response.text(), new RegExp(identity.nonce));
      }
    }
  } finally {
    for (const child of children) if (!child.killed) child.kill();
    rmSync(directory, { recursive: true, force: true });
  }
});

test("collector selectors are rendered-only and snapshot validation bounds ambiguity", () => {
  assert.equal(AV01_PREVIEW_UI.openPreview.includes("invoke"), false);
  assert.equal(AV01_PREVIEW_UI.selectFile.includes("invoke"), false);
  assert.equal(AV01_PREVIEW_SNAPSHOT.includes("__TAURI"), false);
  assert.match(
    previewManualOriginAction("http://127.0.0.1:32123/"),
    /Preview address/,
  );
  assert.throws(
    () => previewManualOriginAction("https://example.test/"),
    /av01-preview-manual-origin-invalid/,
  );
  const snapshot = validatePreviewSnapshot({
    listenerReady: "true",
    phase: "starting",
    targetIdentity: "root:dev",
    startEnabled: false,
    stopVisible: false,
    frameUrls: [],
    notices: ["Preview is starting"],
    collapsed: false,
  });
  assert.equal(snapshot.phase, "starting");
  assert.throws(
    () => validatePreviewSnapshot({ ...snapshot, frameUrls: ["a", "b"] }),
    /av01-preview-snapshot-invalid/,
  );
});

function previewSnapshot(overrides = {}) {
  return {
    listenerReady: "true",
    phase: "active",
    targetIdentity: "project:chat:session",
    startEnabled: false,
    stopVisible: true,
    frameUrls: ["http://127.0.0.1:41234/"],
    notices: [],
    collapsed: false,
    ...overrides,
  };
}

function journeyCdp(snapshots) {
  const expressions = [];
  return {
    expressions,
    async evaluate(expression) {
      expressions.push(expression);
      if (expression === AV01_PREVIEW_SNAPSHOT) {
        const snapshot = snapshots.shift();
        if (!snapshot) throw new Error("test snapshot underflow");
        return snapshot;
      }
      return true;
    },
  };
}

test("collector journey proves same-owner File, Preview, and collapse sleep-resume through rendered tabs", async () => {
  const frame = "http://127.0.0.1:41234/";
  const cdp = journeyCdp([
    previewSnapshot({ startEnabled: true, stopVisible: false, frameUrls: [] }),
    previewSnapshot({
      phase: "starting",
      stopVisible: false,
      frameUrls: [],
      notices: ["Preview is starting"],
    }),
    previewSnapshot({ phase: "active", frameUrls: [frame] }),
    previewSnapshot({ phase: "active", frameUrls: [frame] }),
    previewSnapshot({ phase: "active", frameUrls: [frame], collapsed: true }),
    previewSnapshot({ phase: "active", frameUrls: [frame] }),
  ]);
  const identity = {
    schemaVersion: 1,
    nonce: "c".repeat(64),
    processId: 9123,
    executablePath: "C:\\fixture\\node.exe",
    creationDate: "2026-09-07T12:00:00.000Z",
    sageProcessId: 4100,
  };
  const journey = await runAv01PreviewJourney(
    cdp,
    Date.now() + 2_000,
    "starting-active",
    {
      captureFixtureProcess: async () => identity,
      assertFixtureRetired: async (actual) => {
        assert.equal(actual, identity);
        return { ...identity, absent: true };
      },
    },
  );
  assert.equal(journey.finalPhase, "active");
  assert.equal(
    cdp.expressions.indexOf(AV01_PREVIEW_UI.selectFile) <
      cdp.expressions.indexOf(AV01_PREVIEW_UI.collapse),
    true,
  );
  assert.equal(
    cdp.expressions.indexOf(AV01_PREVIEW_UI.openPreview) <
      cdp.expressions.lastIndexOf(AV01_PREVIEW_UI.openPreview),
    true,
  );
});

test("collector journey preserves a rendered different manual origin through health failure", async () => {
  const manualOrigin = "http://127.0.0.1:43210/";
  const cdp = journeyCdp([
    previewSnapshot({ startEnabled: true, stopVisible: false, frameUrls: [] }),
    previewSnapshot({
      phase: "starting",
      stopVisible: false,
      frameUrls: [],
      notices: ["Preview is starting"],
    }),
    previewSnapshot({
      phase: "healthFailed",
      stopVisible: false,
      frameUrls: [manualOrigin],
      notices: ["Preview health check failed"],
    }),
  ]);
  const journey = await runAv01PreviewJourney(
    cdp,
    Date.now() + 2_000,
    "health-failed",
    { readManualOrigin: async () => manualOrigin },
  );
  assert.equal(journey.finalPhase, "healthFailed");
  assert.equal(journey.manualOrigin, manualOrigin);
  assert.equal(
    cdp.expressions.some((expression) =>
      expression.includes(JSON.stringify(manualOrigin)),
    ),
    true,
  );
});

test("AV01 finalizer rejects escaped receipt topology and incomplete pending artifacts", () => {
  const directory = root();
  try {
    const common = [
      "--pending",
      path.join(directory, "machine-receipt.pending.json"),
      "--final",
      path.join(directory, "machine-receipt.json"),
      "--source-root",
      directory,
      "--stdout",
      path.join(directory, "sage.stdout.log"),
      "--stderr",
      path.join(directory, "sage.stderr.log"),
      "--sage-cleanup",
      Buffer.from(
        '{"role":"sage","identityLost":false,"remainingCount":0}',
      ).toString("base64"),
      "--vite-cleanup",
      Buffer.from(
        '{"role":"vite","identityLost":false,"remainingCount":0}',
      ).toString("base64"),
      "--deadline-ms",
      "30000",
    ];
    assert.equal(parseAv01PreviewFinalizerArguments(common).deadlineMs, 30000);
    for (const invalid of ["+30000", "3e4", "30000.0"]) {
      assert.throws(
        () =>
          parseAv01PreviewFinalizerArguments([...common.slice(0, -1), invalid]),
        /av01-preview-finalizer-arguments-invalid/,
      );
    }
    assert.throws(
      () =>
        parseAv01PreviewFinalizerArguments([
          "pending",
          common[1],
          ...common.slice(2),
        ]),
      /av01-preview-finalizer-arguments-invalid/,
    );
    assert.throws(
      () =>
        parseAv01PreviewFinalizerArguments([
          ...common.slice(0, 4),
          "--final",
          path.join(directory, "other", "machine-receipt.json"),
          ...common.slice(6),
        ]),
      /av01-preview-finalizer-arguments-invalid/,
    );
    assert.throws(
      () => validatePendingAv01PreviewReceipt({ completed: false }, directory),
      /av01-preview-pending-invalid/,
    );
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
