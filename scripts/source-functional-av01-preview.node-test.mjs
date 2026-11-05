import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  appendFileSync,
  closeSync,
  fstatSync,
  mkdirSync,
  mkdtempSync,
  lstatSync,
  openSync,
  readSync,
  readFileSync,
  renameSync,
  rmSync,
  symlinkSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { connect } from "node:net";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";
import { EventEmitter } from "node:events";
import test from "node:test";
import { JSDOM } from "jsdom";

import {
  createAv01PreviewReceipt,
  parseAv01PreviewArguments,
  readAv01PreviewObservationForTest,
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
  fixtureGitIdentity,
  previewManualOriginAction,
  runFixtureProcessHelper,
  runAv01PreviewJourney,
  validatePreviewSnapshot,
} from "./source-functional-av01-preview-probe.mjs";
import {
  ACP04_READ_ONLY_PACKAGE_JSON_PROMPT,
  ACP04_FILE_TAB_SNAPSHOT,
  ACP04_FILE_TURN_SNAPSHOT,
  revealTurnDisclosureExpression,
  runReadOnlyPackageJsonTurn,
  validateReadOnlyFileProof,
} from "./source-functional-acp04-file-turn.mjs";
import { fillPromptExpression } from "./source-functional-acp04-live-probe.mjs";

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

const OBSERVATION_IO = Object.freeze({
  closeSync,
  fstatSync,
  lstatSync,
  openSync,
  readSync,
});
const FIXTURE_NODE_CLOSE_TIMEOUT_MS = 1_000;

function root() {
  return mkdtempSync(path.join(tmpdir(), "sage-av01-preview-"));
}

function fixtureNodeLifecycle(closeTimeoutMs = FIXTURE_NODE_CLOSE_TIMEOUT_MS) {
  const nodes = [];
  return {
    retain(child) {
      const node = { child, closed: false };
      node.close = new Promise((resolve) => {
        child.once("close", () => {
          node.closed = true;
          resolve();
        });
      });
      nodes.push(node);
      return child;
    },
    async terminateAndAwait() {
      for (const node of nodes) {
        if (!node.closed && !node.child.killed) node.child.kill();
      }
      await Promise.all(
        nodes.map(
          (node) =>
            new Promise((resolve, reject) => {
              const timeout = setTimeout(
                () => reject(new Error("AV01 fixture child close unproven")),
                closeTimeoutMs,
              );
              node.close.then(() => {
                clearTimeout(timeout);
                resolve();
              });
            }),
        ),
      );
    },
  };
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

function fixtureGitOptions() {
  return {
    git: {
      path: "C:\\Program Files\\Git\\cmd\\git.exe",
      bytes: 99,
      sha256: SHA,
    },
    fixture: { path: "C:\\fixture-project" },
    output: "C:\\output",
  };
}

function fixtureGitDependencies(outputs, overrides = {}) {
  const calls = [];
  return {
    calls,
    command: "C:\\Windows\\System32\\cmd.exe",
    validateGit: () => undefined,
    hashGit: () => ({ bytes: 99, sha256: SHA }),
    runTool: async (...args) => {
      calls.push(args);
      return outputs.shift();
    },
    ...overrides,
  };
}

test("AV01 fixture Git waits for captured CMD ownership before fixed authenticated reads", async () => {
  const dependencies = fixtureGitDependencies(["", `${GIT}\r\n${GIT}\r\n`]);
  await expectFixtureGit(
    fixtureGitIdentity(fixtureGitOptions(), 999, dependencies),
    { commit: GIT, tree: GIT },
  );
  assert.deepEqual(
    dependencies.calls.map(([command, args, deadline, output, options]) => ({
      command,
      args,
      deadline,
      output,
      options,
    })),
    [
      {
        command: "C:\\Windows\\System32\\cmd.exe",
        args: [
          "/d",
          "/s",
          "/c",
          'set /p "SAGE_AV01_GIT_ACK=" && "C:\\Program Files\\Git\\cmd\\git.exe" status --porcelain=v1 --untracked-files=all',
        ],
        deadline: 999,
        output: "C:\\output",
        options: {
          commandRole: "av01-fixture-read-only-git",
          expectedExecutablePath: "C:\\Windows\\System32\\cmd.exe",
          identityHandshake: "captured-owner\r\n",
          maxOutputBytes: 4096,
          spawnCwd: "C:\\fixture-project",
          windowsVerbatimArguments: true,
        },
      },
      {
        command: "C:\\Windows\\System32\\cmd.exe",
        args: [
          "/d",
          "/s",
          "/c",
          'set /p "SAGE_AV01_GIT_ACK=" && "C:\\Program Files\\Git\\cmd\\git.exe" rev-parse HEAD "HEAD^{tree}"',
        ],
        deadline: 999,
        output: "C:\\output",
        options: {
          commandRole: "av01-fixture-read-only-git",
          expectedExecutablePath: "C:\\Windows\\System32\\cmd.exe",
          identityHandshake: "captured-owner\r\n",
          maxOutputBytes: 4096,
          spawnCwd: "C:\\fixture-project",
          windowsVerbatimArguments: true,
        },
      },
    ],
  );
  assert.equal(
    dependencies.calls
      .flatMap((call) => call[1])
      .join(" ")
      .includes("call "),
    false,
  );
});

test("AV01 fixture Git rejects unsafe or changed bindings before spawning and retains churn rejection", async () => {
  for (const path of ["C:\\Git&bad\\git.exe", "C:\\Git\r\ngit.exe"]) {
    const options = fixtureGitOptions();
    options.git.path = path;
    const dependencies = fixtureGitDependencies([]);
    await assert.rejects(
      fixtureGitIdentity(options, 999, dependencies),
      /fixture-git-invalid/,
    );
    assert.equal(dependencies.calls.length, 0);
  }
  const changed = fixtureGitDependencies([], {
    hashGit: () => ({ bytes: 100, sha256: SHA }),
  });
  await assert.rejects(
    fixtureGitIdentity(fixtureGitOptions(), 999, changed),
    /fixture-git-invalid/,
  );
  assert.equal(changed.calls.length, 0);
  await assert.rejects(
    fixtureGitIdentity(
      fixtureGitOptions(),
      999,
      fixtureGitDependencies(["dirty", `${GIT}\n${GIT}\n`]),
    ),
    /fixture-git-churn/,
  );
});

test("AV01 fixture Git negative matrix fails closed at the intended boundary", async () => {
  const unsafeCharacters = [
    '"',
    "%",
    "!",
    "^",
    "&",
    "|",
    "<",
    ">",
    "(",
    ")",
    "\r",
    "\n",
    "\0",
  ];
  const cases = [
    ...unsafeCharacters.map((character) => ({
      label: `unsafe CMD character ${JSON.stringify(character)}`,
      options: () => {
        const options = fixtureGitOptions();
        options.git.path = `C:\\Git${character}unsafe\\git.exe`;
        return options;
      },
      dependencies: () => fixtureGitDependencies([]),
      error: /fixture-git-invalid/u,
      runs: 0,
    })),
    {
      label: "distinct SHA mismatch",
      options: fixtureGitOptions,
      dependencies: () =>
        fixtureGitDependencies([], {
          hashGit: () => ({ bytes: 99, sha256: "d".repeat(64) }),
        }),
      error: /fixture-git-invalid/u,
      runs: 0,
    },
    {
      label: "post-read binding mutation",
      options: fixtureGitOptions,
      dependencies: () => {
        let validations = 0;
        return fixtureGitDependencies(["", `${GIT}\r\n${GIT}\r\n`], {
          hashGit: () => {
            validations += 1;
            return {
              bytes: 99,
              sha256: validations === 1 ? SHA : "e".repeat(64),
            };
          },
        });
      },
      error: /fixture-git-invalid/u,
      runs: 2,
    },
    {
      label: "malformed identity output",
      options: fixtureGitOptions,
      dependencies: () => fixtureGitDependencies(["", "not-a-git-identity\n"]),
      error: /fixture-git-churn/u,
      runs: 2,
    },
  ];

  for (const entry of cases) {
    const dependencies = entry.dependencies();
    await assert.rejects(
      fixtureGitIdentity(entry.options(), 999, dependencies),
      entry.error,
      entry.label,
    );
    assert.equal(dependencies.calls.length, entry.runs, entry.label);
  }
});

async function expectFixtureGit(promise, expected) {
  assert.deepEqual(await promise, expected);
}

function fixtureHelperChild({
  stdout = "",
  stderr = "",
  code,
  emit = true,
  onKill,
}) {
  const child = new EventEmitter();
  child.stdout = new EventEmitter();
  child.stderr = new EventEmitter();
  child.killed = false;
  child.kill = () => {
    child.killed = true;
    onKill?.(child);
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

test("AV01 fixture lifecycle awaits a queued delayed child close", async () => {
  const nodes = fixtureNodeLifecycle();
  const child = nodes.retain(
    fixtureHelperChild({
      emit: false,
      onKill: (delayed) => {
        queueMicrotask(() => {
          setTimeout(() => delayed.emit("close", 0), 1);
        });
      },
    }),
  );

  await nodes.terminateAndAwait();
  assert.equal(child.killed, true);
});

test("AV01 fixture lifecycle retains its root when close settlement is unproven", async () => {
  const directory = root();
  const nodes = fixtureNodeLifecycle(1);
  const child = nodes.retain(fixtureHelperChild({ emit: false }));
  try {
    await assert.rejects(
      nodes.terminateAndAwait(),
      /AV01 fixture child close unproven/,
    );
    assert.equal(lstatSync(directory).isDirectory(), true);
  } finally {
    child.emit("close", 0);
    await nodes.terminateAndAwait();
    rmSync(directory, { recursive: true, force: true });
  }
});

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
    "git-path": path.join(directory, "git.exe"),
    "git-bytes": "100",
    "git-sha256": SHA,
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

test("wrapper gives AV01 exactly one Git identity triple and keeps it out of ACP-04", () => {
  const wrapper = readFileSync(
    path.resolve("scripts/run-source-functional-native-ide-journey.ps1"),
    "utf8",
  );
  const collectorVectors = wrapper.slice(
    wrapper.lastIndexOf("$sageRuntime = Capture-OwnedRuntimeOrQuarantine"),
  );
  const acp =
    /if \(\$JourneyKind -eq "acp04-live"\) \{\s*\$arguments = @\((?<arguments>[\s\S]*?)\n\s*\)\s*\} elseif \(\$JourneyKind -eq "av01-preview"\)/u.exec(
      collectorVectors,
    )?.groups?.arguments;
  const av01 =
    /\} elseif \(\$JourneyKind -eq "av01-preview"\) \{[\s\S]*?\$arguments = @\((?<arguments>[\s\S]*?)\n\s*\)\s*\} else \{/u.exec(
      collectorVectors,
    )?.groups?.arguments;
  assert.ok(acp, "ACP-04 collector vector is present");
  assert.ok(av01, "AV01 collector vector is present");
  const flags = (body) =>
    [...body.matchAll(/"--(?<flag>[a-z0-9-]+)"/gu)].map(
      (match) => match.groups.flag,
    );
  const gitFlags = ["git-path", "git-bytes", "git-sha256"];
  assert.deepEqual(
    flags(av01).filter((flag) => gitFlags.includes(flag)),
    gitFlags,
  );
  assert.deepEqual(
    flags(acp).filter((flag) => gitFlags.includes(flag)),
    [],
  );
  const misplaced = collectorVectors
    .replace(
      '      "--git-path", $gitExecutable, "--git-bytes", "$gitBytes", "--git-sha256", $gitSha256,\n',
      "",
    )
    .replace(
      '      "--codex-path", $codex, "--codex-bytes", "$codexBytes", "--codex-sha256", $codexSha256,\n',
      '      "--codex-path", $codex, "--codex-bytes", "$codexBytes", "--codex-sha256", $codexSha256,\n      "--git-path", $gitExecutable, "--git-bytes", "$gitBytes", "--git-sha256", $gitSha256,\n',
    );
  const misplacedAv01 =
    /\} elseif \(\$JourneyKind -eq "av01-preview"\) \{[\s\S]*?\$arguments = @\((?<arguments>[\s\S]*?)\n\s*\)\s*\} else \{/u.exec(
      misplaced,
    )?.groups?.arguments;
  assert.ok(misplacedAv01, "misplaced AV01 collector vector is present");
  assert.deepEqual(
    flags(misplacedAv01).filter((flag) => gitFlags.includes(flag)),
    [],
  );

  const directory = root();
  try {
    const vector = argumentsFor(directory);
    assert.equal(
      parseAv01PreviewArguments(vector).git.path,
      path.join(directory, "git.exe"),
    );
    for (const flag of gitFlags) {
      const offset = vector.indexOf(`--${flag}`);
      assert.notEqual(offset, -1);
      assert.throws(
        () =>
          parseAv01PreviewArguments([
            ...vector.slice(0, offset),
            ...vector.slice(offset + 2),
          ]),
        /av01-preview-arguments-invalid/,
      );
    }
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
    writeFileSync(path.join(directory, "av01-preview.png"), "png");
    const options = {
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
    };
    const observation = {
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
      fileProof: {
        schemaVersion: 1,
        turnId: "turn-3",
        itemId: "item-command",
        projectId: "project-a",
        chatId: "chat-a",
        sessionId: "session-a",
        runtimeEpoch: "1",
        rootId: "fixture-root",
        relativePath: "package.json",
        actionEnabled: true,
        selectedReadyFile: true,
        observedAt: "2026-09-07T12:00:30.000Z",
      },
      fixtureBefore: readAv01PreviewFixtureIdentity(fixture, "starting-active"),
      fixtureAfter: readAv01PreviewFixtureIdentity(fixture, "starting-active"),
      gitBefore: { commit: GIT, tree: GIT },
      gitAfter: { commit: GIT, tree: GIT },
      preview: {
        case: "starting-active",
        finalPhase: "active",
        activeFrameUrl: "http://127.0.0.1:41234/",
        manualOrigin: null,
        fixtureProcess: FIXTURE_CAPTURE,
        fixtureRetirement: { ...FIXTURE_CAPTURE, absent: true },
      },
    };
    writeFileSync(
      path.join(directory, "av01-preview-observation.json"),
      `${JSON.stringify(observation, null, 2)}\n`,
    );
    const receipt = createAv01PreviewReceipt(
      options,
      observation,
      { id: "canonical-page" },
      ["av01-preview-observation.json", "av01-preview.png"],
      { connectionCount: 1, maxInFlight: 1, requestCount: 12 },
    );
    assert.equal(
      validatePendingAv01PreviewReceipt(receipt, directory).preview
        .fixtureProcess.processId,
      FIXTURE_CAPTURE.processId,
    );
    const observationArtifact = receipt.artifacts.find(
      ({ name }) => name === "av01-preview-observation.json",
    );
    const screenshotArtifact = receipt.artifacts.find(
      ({ name }) => name === "av01-preview.png",
    );
    const screenshotPath = path.join(directory, "av01-preview.png");
    unlinkSync(screenshotPath);
    assert.throws(
      () =>
        validatePendingAv01PreviewReceipt(
          { ...receipt, artifacts: [observationArtifact, observationArtifact] },
          directory,
        ),
      /av01-preview-pending-invalid/,
    );
    writeFileSync(screenshotPath, "png");
    assert.throws(
      () =>
        validatePendingAv01PreviewReceipt(
          { ...receipt, artifacts: [screenshotArtifact, screenshotArtifact] },
          directory,
        ),
      /av01-preview-pending-invalid/,
    );
    assert.throws(
      () =>
        validatePendingAv01PreviewReceipt(
          { ...receipt, artifacts: [observationArtifact] },
          directory,
        ),
      /av01-preview-pending-invalid/,
    );
    assert.throws(
      () =>
        validatePendingAv01PreviewReceipt(
          {
            ...receipt,
            artifacts: [
              observationArtifact,
              { ...screenshotArtifact, name: "unrecognized-artifact" },
            ],
          },
          directory,
        ),
      /av01-preview-pending-invalid/,
    );
    writeFileSync(screenshotPath, "corrupt-png");
    assert.throws(
      () => validatePendingAv01PreviewReceipt(receipt, directory),
      /av01-preview-pending-invalid/,
    );
    unlinkSync(screenshotPath);
    assert.throws(
      () => validatePendingAv01PreviewReceipt(receipt, directory),
      /av01-preview-pending-invalid/,
    );
    writeFileSync(screenshotPath, "png");
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
    for (const mutate of [
      (value) => ({
        ...value,
        fileProof: { ...value.fileProof, fixtureFilesSha256: SHA },
      }),
      (value) => ({
        ...value,
        fixtureGit: { ...value.fixtureGit, tree: "d".repeat(40) },
      }),
      (value) => ({
        ...value,
        fileProof: {
          ...value.fileProof,
          fixtureGit: { commit: "d".repeat(40), tree: GIT },
        },
      }),
      (value) => ({
        ...value,
        fileProof: { ...value.fileProof, forged: true },
      }),
      (value) => ({
        ...value,
        fileProof: {
          ...value.fileProof,
          observations: {
            ...value.fileProof.observations,
            gitAfter: { commit: "d".repeat(40), tree: GIT },
          },
        },
      }),
    ]) {
      assert.throws(
        () => validatePendingAv01PreviewReceipt(mutate(receipt), directory),
        /av01-preview-pending-invalid/,
      );
    }
    const digest = (value) =>
      createHash("sha256").update(JSON.stringify(value)).digest("hex");
    const forgedFiles = [];
    const forgedGit = { commit: "d".repeat(40), tree: "e".repeat(40) };
    const internallyConsistentFixture = {
      ...receipt,
      fixture: { ...receipt.fixture, files: forgedFiles },
      fileProof: {
        ...receipt.fileProof,
        fixtureFilesSha256: digest(forgedFiles),
        observations: {
          ...receipt.fileProof.observations,
          fixtureBeforeSha256: digest(forgedFiles),
          fixtureAfterSha256: digest(forgedFiles),
        },
      },
    };
    const internallyConsistentGit = {
      ...receipt,
      fixtureGit: forgedGit,
      fileProof: {
        ...receipt.fileProof,
        fixtureGit: forgedGit,
        observations: {
          ...receipt.fileProof.observations,
          gitBefore: forgedGit,
          gitAfter: forgedGit,
        },
      },
    };
    for (const forged of [
      internallyConsistentFixture,
      internallyConsistentGit,
    ]) {
      assert.throws(
        () => validatePendingAv01PreviewReceipt(forged, directory),
        /av01-preview-pending-invalid/,
      );
    }
    for (const mutateObservation of [
      () => ({}),
      () => ({
        ...observation,
        fixtureAfter: { ...observation.fixtureAfter, nonce: SHA },
      }),
      () => ({
        ...observation,
        gitAfter: { commit: SHA.slice(0, 40), tree: GIT },
      }),
      () => ({
        ...observation,
        fileProof: { ...observation.fileProof, rootId: "other-root" },
      }),
    ]) {
      writeFileSync(
        path.join(directory, "av01-preview-observation.json"),
        `${JSON.stringify(mutateObservation())}\n`,
      );
      assert.throws(
        () => validatePendingAv01PreviewReceipt(receipt, directory),
        /av01-preview-pending-invalid/,
      );
    }
    const observationPath = path.join(
      directory,
      "av01-preview-observation.json",
    );
    writeFileSync(observationPath, `${JSON.stringify(observation)}\n`);
    assert.throws(
      () => validatePendingAv01PreviewReceipt(receipt, directory),
      /av01-preview-pending-invalid/,
    );
    writeFileSync(observationPath, "");
    assert.throws(
      () => validatePendingAv01PreviewReceipt(receipt, directory),
      /av01-preview-pending-invalid/,
    );
    const linkedObservation = path.join(directory, "linked-observation.json");
    writeFileSync(linkedObservation, JSON.stringify(observation));
    unlinkSync(observationPath);
    symlinkSync(linkedObservation, observationPath);
    assert.throws(
      () => validatePendingAv01PreviewReceipt(receipt, directory),
      /av01-preview-pending-invalid/,
    );
    unlinkSync(observationPath);
    const serialized = Buffer.from(JSON.stringify(observation));
    const exact = Buffer.concat([
      serialized,
      Buffer.alloc(512 * 1024 - serialized.length, 0x20),
    ]);
    const boundedReceipt = {
      ...receipt,
      artifacts: receipt.artifacts.map((entry) =>
        entry.name === "av01-preview-observation.json"
          ? {
              ...entry,
              bytes: exact.length,
              sha256: createHash("sha256").update(exact).digest("hex"),
            }
          : entry,
      ),
    };
    writeFileSync(observationPath, exact);
    assert.doesNotThrow(() =>
      validatePendingAv01PreviewReceipt(boundedReceipt, directory),
    );
    const over = Buffer.concat([exact, Buffer.from(" ")]);
    writeFileSync(observationPath, over);
    const overReceipt = {
      ...boundedReceipt,
      artifacts: boundedReceipt.artifacts.map((entry) =>
        entry.name === "av01-preview-observation.json"
          ? {
              ...entry,
              bytes: over.length,
              sha256: createHash("sha256").update(over).digest("hex"),
            }
          : entry,
      ),
    };
    assert.throws(
      () => validatePendingAv01PreviewReceipt(overReceipt, directory),
      /av01-preview-pending-invalid/,
    );
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("AV01 observation reads stay descriptor-bound across short reads, growth, and replacement", () => {
  const directory = root();
  const observationPath = path.join(directory, "av01-preview-observation.json");
  try {
    const bytes = Buffer.from('{"bounded":true}\n');
    writeFileSync(observationPath, bytes);
    const declared = {
      name: "av01-preview-observation.json",
      bytes: bytes.length,
      sha256: createHash("sha256").update(bytes).digest("hex"),
    };
    let closes = 0;
    const shortReads = {
      ...OBSERVATION_IO,
      closeSync(descriptor) {
        closes += 1;
        closeSync(descriptor);
      },
      readSync(descriptor, buffer, offset, length, position) {
        return readSync(
          descriptor,
          buffer,
          offset,
          Math.min(length, 3),
          position,
        );
      },
    };
    assert.deepEqual(
      readAv01PreviewObservationForTest(directory, declared, shortReads),
      { bounded: true },
    );
    assert.equal(closes, 1);

    let grew = false;
    const growth = {
      ...OBSERVATION_IO,
      readSync(descriptor, buffer, offset, length, position) {
        const count = readSync(
          descriptor,
          buffer,
          offset,
          Math.min(length, 3),
          position,
        );
        if (!grew) {
          grew = true;
          appendFileSync(observationPath, "x");
        }
        return count;
      },
    };
    assert.throws(
      () => readAv01PreviewObservationForTest(directory, declared, growth),
      /av01-preview-pending-invalid/,
    );

    writeFileSync(observationPath, bytes);
    let replaced = false;
    const replacement = {
      ...OBSERVATION_IO,
      readSync(descriptor, buffer, offset, length, position) {
        const count = readSync(
          descriptor,
          buffer,
          offset,
          Math.min(length, 3),
          position,
        );
        if (!replaced) {
          replaced = true;
          const retired = path.join(directory, "retired-observation.json");
          renameSync(observationPath, retired);
          writeFileSync(observationPath, bytes);
        }
        return count;
      },
    };
    assert.throws(
      () => readAv01PreviewObservationForTest(directory, declared, replacement),
      /av01-preview-pending-invalid/,
    );

    writeFileSync(observationPath, bytes);
    let errorCloses = 0;
    const readError = {
      ...OBSERVATION_IO,
      closeSync(descriptor) {
        errorCloses += 1;
        closeSync(descriptor);
      },
      readSync() {
        throw new Error("injected read failure");
      },
    };
    assert.throws(
      () => readAv01PreviewObservationForTest(directory, declared, readError),
      /av01-preview-pending-invalid/,
    );
    assert.equal(errorCloses, 1);

    writeFileSync(observationPath, bytes);
    let overflowed = false;
    const overflow = {
      ...OBSERVATION_IO,
      readSync(descriptor, buffer, offset, length, position) {
        const count = readSync(
          descriptor,
          buffer,
          offset,
          Math.min(length, 3),
          position,
        );
        if (!overflowed) {
          overflowed = true;
          appendFileSync(observationPath, Buffer.alloc(512 * 1024, 0x20));
        }
        return count;
      },
    };
    assert.throws(
      () => readAv01PreviewObservationForTest(directory, declared, overflow),
      /av01-preview-pending-invalid/,
    );
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("collector third turn dispatches only one typed package.json File action and requires its ready owner tab", async () => {
  const owner = {
    phase: "complete",
    projectId: "project-a",
    chatId: "chat-a",
    sessionId: "session-a",
    runtimeEpoch: "1",
    turnIds: ["turn-1", "turn-2"],
  };
  const turns = (third = null) => [
    {
      id: "turn-1",
      sessionId: "session-a",
      runtimeEpoch: "1",
      ordinal: "1",
      state: "Completed",
      items: [],
      actions: [],
    },
    {
      id: "turn-2",
      sessionId: "session-a",
      runtimeEpoch: "1",
      ordinal: "2",
      state: "Completed",
      items: [],
      actions: [],
    },
    ...(third ? [third] : []),
  ];
  const completed = {
    id: "turn-3",
    sessionId: "session-a",
    runtimeEpoch: "1",
    ordinal: "3",
    state: "Completed",
    items: [
      {
        id: "item-user",
        role: "user",
        readOnlyPrompt: true,
      },
    ],
    actions: [
      {
        itemId: "item-command",
        rootId: "fixture-root",
        relativePath: "package.json",
        enabled: true,
      },
    ],
  };
  const snapshots = [
    {
      projectId: "project-a",
      chatId: "chat-a",
      currentSessionId: "session-a",
      expectedRootId: "fixture-root",
      projectionRevision: "2",
      composerEnabled: true,
      failures: [],
      turns: turns(),
    },
    {
      projectId: "project-a",
      chatId: "chat-a",
      currentSessionId: "session-a",
      expectedRootId: "fixture-root",
      projectionRevision: "3",
      composerEnabled: true,
      failures: [],
      turns: turns(),
    },
    {
      projectId: "project-a",
      chatId: "chat-a",
      currentSessionId: "session-a",
      expectedRootId: "fixture-root",
      projectionRevision: "3",
      composerEnabled: true,
      failures: [],
      turns: turns(completed),
    },
    {
      projectId: "project-a",
      chatId: "chat-a",
      currentSessionId: "session-a",
      expectedRootId: "fixture-root",
      projectionRevision: "3",
      composerEnabled: true,
      failures: [],
      turns: turns(completed),
    },
  ];
  const expressions = [];
  const proof = await runReadOnlyPackageJsonTurn(
    {
      async evaluate(expression) {
        expressions.push(expression);
        if (expression === ACP04_FILE_TURN_SNAPSHOT) return snapshots.shift();
        if (expression === ACP04_FILE_TAB_SNAPSHOT)
          return {
            projectId: "project-a",
            chatId: "chat-a",
            sessionId: "session-a",
            runtimeEpoch: "1",
            ready: true,
            tabs: [
              {
                rootId: "fixture-root",
                relativePath: "package.json",
                selected: true,
              },
            ],
          };
        return true;
      },
    },
    Date.now() + 2_000,
    owner,
  );
  assert.equal(proof.turnId, "turn-3");
  assert.equal(proof.relativePath, "package.json");
  assert.equal(
    ACP04_READ_ONLY_PACKAGE_JSON_PROMPT,
    "Use exec_command exactly once with cmd `cat package.json`, shell `C:\\Program Files\\Git\\bin\\bash.exe`, and login `false`. Read package.json only. Do not modify files or run unrelated commands.",
  );
  assert.equal(
    expressions.filter(
      (expression) =>
        expression ===
        fillPromptExpression(ACP04_READ_ONLY_PACKAGE_JSON_PROMPT),
    ).length,
    1,
  );
  assert.equal(expressions.includes(ACP04_FILE_TAB_SNAPSHOT), true);
  assert.throws(
    () => validateReadOnlyFileProof({ ...proof, rootId: "" }),
    /acp04-file-turn-proof-invalid/,
  );
});

test("collector rejects candidate roots, duplicate actions, and post-click owner epoch churn", async () => {
  const owner = {
    phase: "complete",
    projectId: "project-a",
    chatId: "chat-a",
    sessionId: "session-a",
    runtimeEpoch: "1",
    turnIds: ["turn-1", "turn-2"],
  };
  const snapshot = (actions) => ({
    projectId: owner.projectId,
    chatId: owner.chatId,
    currentSessionId: owner.sessionId,
    expectedRootId: "fixture-root",
    projectionRevision: "3",
    composerEnabled: true,
    failures: [],
    turns: [
      ...["turn-1", "turn-2"].map((id, index) => ({
        id,
        sessionId: owner.sessionId,
        runtimeEpoch: owner.runtimeEpoch,
        ordinal: String(index + 1),
        state: "Completed",
        items: [],
        actions: [],
      })),
      {
        id: "turn-3",
        sessionId: owner.sessionId,
        runtimeEpoch: owner.runtimeEpoch,
        ordinal: "3",
        state: "Completed",
        items: [{ id: "user-3", role: "user", readOnlyPrompt: true }],
        actions,
      },
    ],
  });
  const action = (rootId = "fixture-root") => ({
    itemId: "command-3",
    rootId,
    relativePath: "package.json",
    enabled: true,
  });
  const cdpFor = (snapshots, tab) => ({
    async evaluate(expression) {
      if (expression === ACP04_FILE_TURN_SNAPSHOT) return snapshots.shift();
      if (expression === ACP04_FILE_TAB_SNAPSHOT) return tab;
      return true;
    },
  });
  for (const [actions, error] of [
    [[action("foreign-root")], /acp04-file-turn-resource-mismatch/],
    [
      [action(), { ...action(), itemId: "command-4" }],
      /acp04-file-turn-action-ambiguous/,
    ],
  ]) {
    await assert.rejects(
      runReadOnlyPackageJsonTurn(
        cdpFor([snapshot([]), snapshot(actions)]),
        Date.now() + 100,
        owner,
      ),
      error,
    );
  }
  await assert.rejects(
    runReadOnlyPackageJsonTurn(
      cdpFor([snapshot([]), snapshot([action()]), snapshot([action()])], {
        projectId: owner.projectId,
        chatId: owner.chatId,
        sessionId: owner.sessionId,
        runtimeEpoch: "2",
        ready: true,
        tabs: [
          {
            rootId: "fixture-root",
            relativePath: "package.json",
            selected: true,
          },
        ],
      }),
      // This assertion needs time for the post-click tab snapshot, not a
      // production journey deadline. Keep it bounded while allowing the
      // intended owner/epoch check to run deterministically under load.
      Date.now() + 1_000,
      owner,
    ),
    /file-turn-owner-churn/,
  );
  await assert.rejects(
    runReadOnlyPackageJsonTurn(cdpFor([], null), Date.now() + 100, {
      ...owner,
      runtimeEpoch: "123456789012345678901",
    }),
    /acp04-file-turn-mode-a-invalid/,
  );
});

test("healthy fixture delays then serves its nonce marker; failed and exited cases do not fabricate it", async () => {
  const directory = root();
  const nodes = fixtureNodeLifecycle();
  try {
    for (const previewCase of AV01_PREVIEW_CASES) {
      const fixture = path.join(directory, previewCase);
      mkdirSync(fixture, { recursive: true });
      const identity = createAv01PreviewFixture({
        fixture,
        previewCase,
        nonce: "2".repeat(64),
      });
      const child = nodes.retain(
        spawn(process.execPath, ["preview-fixture.mjs"], {
          cwd: fixture,
          stdio: ["ignore", "pipe", "pipe"],
        }),
      );
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
    await nodes.terminateAndAwait();
    rmSync(directory, { recursive: true, force: true });
  }
});

test("collector selectors are rendered-only and snapshot validation bounds ambiguity", () => {
  assert.equal(AV01_PREVIEW_UI.openPreview.includes("invoke"), false);
  assert.equal(AV01_PREVIEW_UI.selectFile.includes("invoke"), false);
  assert.equal(AV01_PREVIEW_SNAPSHOT.includes("__TAURI"), false);
  assert.equal(AV01_PREVIEW_SNAPSHOT.includes("innerText"), false);
  assert.equal(ACP04_FILE_TURN_SNAPSHOT.includes("[..."), false);
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

// JSDOM stand-in for the CDP page: expressions run against markup copied
// from main's rendered components.
async function withPage(markup, run) {
  const dom = new JSDOM(`<!doctype html><body>${markup}</body>`);
  const names = [
    "document",
    "Event",
    "HTMLTextAreaElement",
    "HTMLInputElement",
    "NodeFilter",
  ];
  const previous = Object.fromEntries(
    names.map((name) => [name, globalThis[name]]),
  );
  for (const name of names) globalThis[name] = dom.window[name];
  try {
    return await run(dom.window.document, {
      evaluate: async (expression) => eval(expression),
    });
  } finally {
    Object.assign(globalThis, previous);
    dom.window.close();
  }
}

function fileTurnMarkup({ id, ordinal, items = "" }) {
  return `<section class="acp-turn" data-turn-state="completed" aria-label="Turn ${ordinal}" data-turn-id="${id}" data-session-id="session-a" data-runtime-epoch="1" data-turn-ordinal="${ordinal}"><header class="acp-turn__header"><span class="acp-state acp-turn__state" role="status" aria-label="Completed" data-turn-status="${id}">Completed</span></header><div class="acp-turn__items">${items}</div></section>`;
}

const RENDERED_READ_ONLY_PROMPT =
  "<p>Use exec_command exactly once with cmd <code>cat package.json</code>, shell <code>C:\\Program Files\\Git\\bin\\bash.exe</code>, and login <code>false</code>. Read package.json only. Do not modify files or run unrelated commands.</p>";

function readOnlyAgentMarkup() {
  return `<main class="workbench workbench--agent"><div class="agent-chat-center"><div id="agent-chat-panel" aria-label="Agent chat panes" class="agent-chat-panes" role="tabpanel"><section class="agent-chat-pane agent-chat-pane--focused" aria-label="Chat pane Fixture" data-chat-id="chat-a"><section class="chat-surface chat-surface--agent" aria-label="Agent chat"><section class="acp-chat-projection" aria-label="Agent chat Fixture" data-projection-revision="2" data-project-id="project-a" data-chat-id="chat-a" data-current-session-id="session-a" data-expected-resource-root-id="fixture-root">${fileTurnMarkup({ id: "turn-1", ordinal: 1 })}${fileTurnMarkup({ id: "turn-2", ordinal: 2 })}</section><form class="composer"><textarea id="prompt-chat-a" aria-label="Message agent"></textarea><button class="send-button" type="submit" aria-label="Send message"></button></form></section></section></div></div><div class="agent-artifact-surface"><aside id="agent-artifacts" class="agent-artifact-pane" aria-label="Agent artifacts" data-owner-project-id="project-a" data-owner-chat-id="chat-a" data-owner-session-id="session-a" data-owner-runtime-epoch="1"><header class="agent-artifact-pane__header"><nav class="tab-strip" aria-label="Agent artifact tabs"></nav><button class="agent-artifact-pane__collapse" type="button" aria-label="Collapse files, changes, and preview"></button></header><div class="agent-artifact-pane__canvas"></div></aside></div></main>`;
}

// Mirrors ChatTranscriptTurnSummary (folded "Worked for …") and
// ToolActivityRollup (folded tool row) for a finished Turn.
function mountReadOnlyTurn(document, clicks, rollups = 1) {
  const projection = document.querySelector("section.acp-chat-projection");
  projection.dataset.projectionRevision = "3";
  projection.insertAdjacentHTML(
    "beforeend",
    fileTurnMarkup({
      id: "turn-3",
      ordinal: 3,
      items: `<article class="acp-message acp-message--user" data-item-id="item-user" data-message-role="user"><header class="acp-message__meta"><span class="acp-message__role">You</span></header><div class="acp-markdown">${RENDERED_READ_ONLY_PROMPT}</div></article><details class="chat-transcript-work"><summary>Worked for 2s<svg aria-hidden="true"></svg></summary></details><article class="acp-message acp-message--assistant" data-item-id="item-answer" data-message-role="assistant"><div class="acp-markdown"><p>Read package.json.</p></div></article>`,
    }),
  );
  const worked = projection.querySelector("details.chat-transcript-work");
  worked.querySelector("summary").addEventListener("click", (event) => {
    event.preventDefault();
    clicks.worked += 1;
    worked.open = true;
    worked.insertAdjacentHTML(
      "afterend",
      `<div class="chat-transcript-work__groups">${'<details class="acp-activity-card acp-tool-rollup" data-activity-kind="read" data-activity-state="completed"><summary aria-label="Read package.json, Completed">Read package.json</summary></details>'.repeat(rollups)}</div>`,
    );
    for (const rollup of document.querySelectorAll("details.acp-tool-rollup"))
      rollup.querySelector("summary").addEventListener("click", (toggle) => {
        toggle.preventDefault();
        clicks.rollup += 1;
        rollup.open = true;
        rollup.insertAdjacentHTML(
          "beforeend",
          '<div class="acp-tool-rollup__items"><article class="acp-activity-card acp-tool-compact" data-acp-item-id="item-read" data-activity-kind="read" data-activity-state="completed" role="group" aria-label="Read package.json"><ul class="acp-resource-list" aria-label="Referenced resources"><li><button type="button" class="acp-resource-action" aria-label="Open file package.json" data-resource-root-id="fixture-root" data-resource-relative-path="package.json"><span>package.json</span></button></li></ul></article></div>',
        );
        rollup
          .querySelector("button.acp-resource-action")
          .addEventListener("click", () => {
            clicks.action += 1;
            document
              .querySelector('nav[aria-label="Agent artifact tabs"]')
              .insertAdjacentHTML(
                "beforeend",
                '<div class="tab tab--active" role="presentation" data-resource-kind="file"><button class="tab__activate" type="button" role="tab" aria-selected="true" aria-label="File package.json" data-resource-root-id="fixture-root" data-resource-relative-path="package.json"></button><button class="tab__close" type="button" aria-label="Close package.json"></button></div>',
              );
            document
              .querySelector(".agent-artifact-pane__canvas")
              .insertAdjacentHTML(
                "beforeend",
                '<section class="file-surface resource-document" aria-label="File editor package.json"><div class="file-editor" aria-label="Edit package.json" contenteditable="true"></div></section>',
              );
          });
      });
  });
}

const READ_ONLY_OWNER = Object.freeze({
  phase: "complete",
  projectId: "project-a",
  chatId: "chat-a",
  sessionId: "session-a",
  runtimeEpoch: "1",
  turnIds: ["turn-1", "turn-2"],
});

test("collector reveals the folded Worked read and opens its one package.json File against current transcript markup", async () => {
  await withPage(readOnlyAgentMarkup(), async (document, cdp) => {
    const clicks = { send: 0, worked: 0, rollup: 0, action: 0 };
    document
      .querySelector('button[aria-label="Send message"]')
      .addEventListener("click", (event) => {
        event.preventDefault();
        clicks.send += 1;
        mountReadOnlyTurn(document, clicks);
      });
    const proof = await runReadOnlyPackageJsonTurn(
      cdp,
      Date.now() + 5_000,
      READ_ONLY_OWNER,
    );
    assert.deepEqual(clicks, { send: 1, worked: 1, rollup: 1, action: 1 });
    assert.equal(
      document.querySelector('textarea[aria-label="Message agent"]').value,
      ACP04_READ_ONLY_PACKAGE_JSON_PROMPT,
    );
    assert.equal(proof.turnId, "turn-3");
    assert.equal(proof.itemId, "item-read");
    assert.equal(proof.rootId, "fixture-root");
    assert.equal(proof.relativePath, "package.json");
    assert.equal(proof.selectedReadyFile, true);
  });
});

test("collector refuses an ambiguous folded tool row without opening a File", async () => {
  await withPage(readOnlyAgentMarkup(), async (document, cdp) => {
    const clicks = { send: 0, worked: 0, rollup: 0, action: 0 };
    document
      .querySelector('button[aria-label="Send message"]')
      .addEventListener("click", (event) => {
        event.preventDefault();
        clicks.send += 1;
        mountReadOnlyTurn(document, clicks, 2);
      });
    await assert.rejects(
      () =>
        runReadOnlyPackageJsonTurn(cdp, Date.now() + 5_000, READ_ONLY_OWNER),
      /acp04-file-turn-reveal-ambiguous/,
    );
    assert.deepEqual(clicks, { send: 1, worked: 1, rollup: 0, action: 0 });
    assert.equal(
      eval(revealTurnDisclosureExpression("turn-3", "worked")),
      "open",
    );
    assert.equal(
      eval(revealTurnDisclosureExpression("turn-missing", "worked")),
      "ambiguous",
    );
    assert.throws(
      () => revealTurnDisclosureExpression("turn-3", "other"),
      /acp04-file-turn-reveal-invalid/,
    );
  });
});

const PREVIEW_ORIGIN = "http://127.0.0.1:41234/";

function previewStackMarkup({ phase, frame = false, lifecycle = "" }) {
  const control =
    phase === "active"
      ? '<button type="button" class="button button--compact button--danger">Stop</button>'
      : `<button type="button" class="button button--compact button--primary"${phase === "idle" ? "" : " disabled"}>Start</button>`;
  return `<div class="agent-preview-stack" data-preview-listener-ready="true" data-preview-phase="${phase}"${phase === "idle" ? "" : ' data-preview-target-identity="fixture-root:dev"'}>${lifecycle}<div class="agent-preview-targets" role="group" aria-label="Preview targets"><div class="agent-preview-target"><span class="agent-preview-target__identity"><span class="agent-preview-target__name" title="dev">dev</span></span>${control}</div></div><section class="harness-browser" aria-label="Sage browser"><header class="harness-browser__chrome"><span class="harness-browser__connection" aria-label="${frame ? "Loopback preview active" : "No preview connected"}" title="" role="status"><span aria-hidden="true"></span><span class="harness-browser__connection-label">${frame ? "Local" : "Idle"}</span></span><button class="harness-browser__controls-trigger" type="button" aria-label="Preview controls"></button><form class="harness-browser__address"><input aria-label="Preview address" value=""></form></header>${frame ? `<iframe class="harness-browser__frame" title="Sage browser" src="${PREVIEW_ORIGIN}"></iframe>` : '<div class="resource-state resource-state--idle" role="status" aria-atomic="true"><strong class="resource-state__title">Preview is idle</strong><span class="resource-state__detail">Preview shows loopback development servers started from this Chat.</span></div>'}</section></div>`;
}

const STARTING_LIFECYCLE =
  '<div class="agent-preview-lifecycle agent-preview-lifecycle--working agent-preview-lifecycle--compact" role="status" aria-atomic="true"><span class="agent-preview-lifecycle__icon" aria-hidden="true"></span><span class="agent-preview-lifecycle__copy"><span class="agent-preview-lifecycle__line"><strong>Starting process</strong></span><span class="agent-preview-lifecycle__detail sr-only">Launching the selected Preview process.</span></span></div>';

function previewPaneMarkup(stack) {
  return `<div class="agent-artifact-surface"><aside id="agent-artifacts" class="agent-artifact-pane agent-artifact-pane--with-header" aria-label="Agent artifacts"><header class="agent-artifact-pane__header"><nav class="tab-strip" aria-label="Agent artifact tabs"><div class="tab" role="presentation" data-resource-kind="file"><button class="tab__activate" type="button" role="tab" aria-selected="false" aria-label="File package.json" data-resource-root-id="fixture-root" data-resource-relative-path="package.json"></button><button class="tab__close" type="button" aria-label="Close package.json"></button></div><div class="tab tab--active" role="presentation" data-resource-kind="preview"><button class="tab__activate" type="button" role="tab" aria-selected="true" aria-label="Preview" title="Preview"></button></div></nav><button class="agent-artifact-pane__collapse" type="button" aria-label="Collapse files, changes, and preview" title="Collapse resources"></button></header><div class="agent-artifact-pane__canvas">${stack}</div></aside></div>`;
}

function previewWorkbenchMarkup(stack) {
  return `<main class="workbench workbench--agent"><header class="title-bar"><div class="title-bar__modes" role="radiogroup" aria-label="Window mode"><label class="title-bar__mode"><input name="workbench-window-mode" type="radio" value="agent" checked>Agent</label><label class="title-bar__mode"><input name="workbench-window-mode" type="radio" value="editor">Editor</label></div></header><div class="agent-chat-center"><div class="agent-chat-center__header"><div class="agent-artifact-header-slot"></div></div></div><div id="artifact-host">${previewPaneMarkup(stack)}</div></main>`;
}

test("collector Preview selectors drive the current Preview stack, collapse edge action, and Editor exit", async () => {
  await withPage(
    previewWorkbenchMarkup(previewStackMarkup({ phase: "idle" })),
    async (document, cdp) => {
      const read = async () =>
        validatePreviewSnapshot(await cdp.evaluate(AV01_PREVIEW_SNAPSHOT));
      const canvas = () =>
        document.querySelector(".agent-artifact-pane__canvas");
      assert.equal(await cdp.evaluate(AV01_PREVIEW_UI.openPreview), true);
      assert.equal(await cdp.evaluate(AV01_PREVIEW_UI.previewReady), true);
      assert.equal(await cdp.evaluate(AV01_PREVIEW_UI.startEnabled), true);
      assert.deepEqual(await read(), {
        listenerReady: "true",
        phase: "idle",
        targetIdentity: null,
        startEnabled: true,
        stopVisible: false,
        frameUrls: [],
        notices: [
          "Idle",
          "Preview is idlePreview shows loopback development servers started from this Chat.",
        ],
        collapsed: false,
      });

      let starts = 0;
      document
        .querySelector('[aria-label="Preview targets"] button')
        .addEventListener("click", () => {
          starts += 1;
        });
      assert.equal(await cdp.evaluate(AV01_PREVIEW_UI.clickStart), true);
      assert.equal(starts, 1);
      canvas().innerHTML = previewStackMarkup({
        phase: "starting",
        lifecycle: STARTING_LIFECYCLE,
      });
      assert.equal(await cdp.evaluate(AV01_PREVIEW_UI.startEnabled), false);
      assert.equal(await cdp.evaluate(AV01_PREVIEW_UI.clickStart), false);
      const starting = await read();
      assert.equal(starting.phase, "starting");
      assert.equal(starting.targetIdentity, "fixture-root:dev");
      assert.equal(
        starting.notices.some((notice) => notice.includes("Preview")),
        true,
      );
      assert.deepEqual(starting.notices.slice(0, 1), [
        "Starting processLaunching the selected Preview process.",
      ]);

      canvas().innerHTML = previewStackMarkup({ phase: "active", frame: true });
      const active = await read();
      assert.equal(active.stopVisible, true);
      assert.deepEqual(active.frameUrls, [PREVIEW_ORIGIN]);

      const fileTab = document.querySelector(
        '[role="tab"][aria-label^="File "]',
      );
      fileTab.addEventListener("click", () => {
        fileTab.setAttribute("aria-selected", "true");
        document
          .querySelector('[role="tab"][aria-label="Preview"]')
          .setAttribute("aria-selected", "false");
        canvas().innerHTML = "";
      });
      assert.equal(await cdp.evaluate(AV01_PREVIEW_UI.selectFile), true);
      assert.equal(await cdp.evaluate(AV01_PREVIEW_UI.fileSelected), true);
      assert.equal(await cdp.evaluate(AV01_PREVIEW_UI.previewReady), false);
      assert.equal((await read()).phase, "none");

      // The collapsed pane leaves only its edge action in the Chat header.
      assert.equal(await cdp.evaluate(AV01_PREVIEW_UI.reopen), false);
      const host = document.getElementById("artifact-host");
      const slot = document.querySelector(".agent-artifact-header-slot");
      document
        .querySelector(
          'button[aria-label="Collapse files, changes, and preview"]',
        )
        .addEventListener("click", () => {
          host.innerHTML = "";
          slot.innerHTML =
            '<button class="agent-artifact-pane__edge-action" type="button" aria-label="Show files and preview · Ready · 2 resources" aria-controls="agent-artifacts" aria-expanded="false" title="Show files and preview · Ready · 2 resources"></button>';
          slot.firstElementChild.addEventListener("click", () => {
            slot.innerHTML = "";
            host.innerHTML = previewPaneMarkup(
              previewStackMarkup({ phase: "active", frame: true }),
            );
          });
        });
      assert.equal(await cdp.evaluate(AV01_PREVIEW_UI.collapse), true);
      assert.equal((await read()).collapsed, true);
      assert.equal(await cdp.evaluate(AV01_PREVIEW_UI.collapse), false);
      slot.append(slot.firstElementChild.cloneNode(true));
      assert.equal(await cdp.evaluate(AV01_PREVIEW_UI.reopen), false);
      assert.throws(
        () => validatePreviewSnapshot(eval(AV01_PREVIEW_SNAPSHOT)),
        /av01-preview-snapshot-invalid/,
      );
      slot.lastElementChild.remove();
      assert.equal(await cdp.evaluate(AV01_PREVIEW_UI.reopen), true);
      const reopened = await read();
      assert.equal(reopened.collapsed, false);
      assert.deepEqual(reopened.frameUrls, [PREVIEW_ORIGIN]);

      canvas().insertAdjacentHTML(
        "beforeend",
        previewStackMarkup({ phase: "active", frame: true }),
      );
      assert.equal(await cdp.evaluate(AV01_PREVIEW_UI.previewReady), false);
      assert.throws(
        () => validatePreviewSnapshot(eval(AV01_PREVIEW_SNAPSHOT)),
        /av01-preview-snapshot-invalid/,
      );

      let editorClicks = 0;
      document
        .querySelector('input[value="editor"]')
        .addEventListener("click", () => {
          editorClicks += 1;
        });
      assert.equal(await cdp.evaluate(AV01_PREVIEW_UI.leaveAgent), true);
      assert.equal(editorClicks, 1);
    },
  );
});
