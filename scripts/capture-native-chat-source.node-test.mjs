import assert from "node:assert/strict";
import test from "node:test";
import path from "node:path";
import { EventEmitter } from "node:events";
import { createHash } from "node:crypto";
import {
  mkdtempSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
  rmSync,
  existsSync,
  unlinkSync,
} from "node:fs";
import { parseArgs } from "./capture-native-chat-contract.mjs";
import { freshRoots, cleanupRoots } from "./capture-native-chat-process.mjs";
import { captureTiming, cleanupCapture } from "./capture-native-chat-drive.mjs";
import {
  trackedIndexParser,
  readTrackedIndex,
} from "./capture-native-chat-index.mjs";
import {
  stagingPaths,
  parsePromotionArgs,
  promoteCapture,
  persistCapture,
} from "./capture-native-chat-evidence.mjs";
import {
  cleanSource,
  verifySource,
  viteLaunch,
  waitForVite,
  servedAssetHash,
  buildCheckout,
  COLD_ASSET_TIMEOUT_MS,
} from "./capture-native-chat-source.mjs";

const workspace = path.resolve("synthetic-checkout");
const source = "a".repeat(40);
test("app exit waits for trailing stderr before freezing its safe reason", async () => {
  const { watchOwnedApp } = await import("./capture-native-chat-lifecycle.mjs");
  const candidate = child();
  const monitor = watchOwnedApp(candidate);
  try {
    const pending = monitor
      .run(() => new Promise(() => {}))
      .catch((error) => error);
    candidate.exitCode = 101;
    candidate.emit("exit", 101, null);
    await new Promise((resolve) => setTimeout(resolve, 5));
    candidate.stderr.emit(
      "data",
      "curated harness metadata must satisfy its transport contract",
    );
    candidate.emit("close", 101, null);
    assert.equal((await pending).appExit.reason, "harness-transport-contract");
  } finally {
    monitor.dispose();
  }
});
test("owned app output overflow blocks a successful in-flight native operation", async () => {
  const { watchOwnedApp } = await import("./capture-native-chat-lifecycle.mjs");
  const candidate = child();
  const monitor = watchOwnedApp(candidate);
  try {
    await assert.rejects(
      monitor.run(async () => {
        candidate.stdout.emit("data", Buffer.alloc(65537));
        return true;
      }),
      /owned-app-output-bound/u,
    );
  } finally {
    monitor.dispose();
  }
});
test("app exit after CDP discovery supersedes tool failure with bounded private-safe diagnostics", async () => {
  const { watchOwnedApp } = await import("./capture-native-chat-lifecycle.mjs");
  const candidate = child();
  const monitor = watchOwnedApp(candidate);
  const pending = monitor.run(() => new Promise(() => {}));
  candidate.stderr.emit(
    "data",
    "private-sentinel C:\\Users\\private\\sage.rs\ncurated harness metadata must satisfy its transport contract: an available ACP harness requires a binding ID\n",
  );
  candidate.exitCode = 101;
  candidate.emit("exit", 101, null);
  const failure = await pending.catch((error) => error);
  assert.equal(failure.message, "owned-app-exited");
  assert.deepEqual(failure.appExit, {
    code: 101,
    signal: null,
    reason: "harness-transport-contract",
  });
  assert.equal(JSON.stringify(failure.appExit).includes("private"), false);
  assert.equal(monitor.failure().appExit.reason, "harness-transport-contract");
  assert.equal(candidate.listenerCount("exit"), 0);
  monitor.dispose();
  assert.equal(candidate.stderr.listenerCount("data"), 0);
});
test("profile cleanup retries Windows locks after a partially removed root", async () => {
  const root = mkdtempSync(path.join(process.cwd(), ".nc1-cleanup-test-"));
  try {
    const roots = freshRoots(root);
    let attempts = 0;
    const delays = [];
    await cleanupRoots(roots, {
      remove(directory, options) {
        assert.equal(directory, roots.owned);
        assert.deepEqual(options, { recursive: true });
        attempts += 1;
        if (attempts < 3) {
          if (attempts === 1) rmSync(roots.profile, { recursive: true });
          throw Object.assign(new Error("profile is locked"), {
            code: attempts === 1 ? "EPERM" : "EBUSY",
            syscall: "rmdir",
            path: roots.profile,
          });
        }
        rmSync(directory, options);
      },
      pause: async (delay) => delays.push(delay),
    });
    assert.equal(attempts, 3);
    assert.deepEqual(delays, [500, 500]);
    assert.equal(existsSync(roots.owned), false);
    await cleanupRoots(roots);
  } finally {
    rmSync(root, { recursive: true });
  }
});
test("exhausted profile cleanup saves the real filesystem error without an absolute path", async () => {
  const root = mkdtempSync(path.join(process.cwd(), ".nc1-cleanup-test-"));
  try {
    const roots = freshRoots(root);
    const timing = captureTiming();
    const original = Object.assign(
      new Error("operation not permitted: " + roots.profile),
      {
        code: "EPERM",
        syscall: "rmdir",
        path: path.join(roots.profile, "EBWebView"),
      },
    );
    let attempts = 0;
    const delays = [];
    const error = await cleanupCapture(timing.record, [
      {
        step: "roots-cleanup",
        action: () =>
          cleanupRoots(roots, {
            remove() {
              attempts += 1;
              throw original;
            },
            pause: async (delay) => delays.push(delay),
          }),
      },
    ]);
    assert.equal(attempts, 21);
    assert.deepEqual(delays, Array(20).fill(500));
    assert.equal(error.cause, original);
    let probe;
    const result = persistCapture(
      {
        output: { probe: "blocked.probe.json" },
        options: parseArgs(captureArgs),
        run,
        directory: "ignored",
        timing,
        elapsedMs: 100,
        error,
      },
      (_file, bytes) => {
        probe = JSON.parse(bytes);
      },
    );
    assert.equal(result.reason, "roots-cleanup-failed");
    assert.deepEqual(probe.cleanupError, {
      code: "EPERM",
      syscall: "rmdir",
      path: "profile/EBWebView",
      attempts: 21,
    });
    assert.equal(JSON.stringify(probe).includes(roots.owned), false);
    assert.equal(probe.timing.steps[1].step, "roots-cleanup-failed");
  } finally {
    rmSync(root, { recursive: true });
  }
});
test("profile cleanup rejects unsafe roots and never retries non-lock errors", async () => {
  const root = mkdtempSync(path.join(process.cwd(), ".nc1-cleanup-test-"));
  try {
    const roots = freshRoots(root);
    let attempts = 0;
    const delays = [];
    const boundary = {
      remove() {
        attempts += 1;
        throw Object.assign(new Error("io failure"), {
          code: "EIO",
          syscall: "unlink",
        });
      },
      pause: async (delay) => delays.push(delay),
    };
    await assert.rejects(
      cleanupRoots({ ...roots, owned: root }, boundary),
      /cleanup-containment/u,
    );
    assert.equal(attempts, 0);
    await assert.rejects(cleanupRoots(roots, boundary), (error) => {
      assert.equal(error.message, "roots-cleanup-failed");
      assert.deepEqual(error.cleanupError, {
        code: "EIO",
        syscall: "unlink",
        path: "owned-root",
        attempts: 1,
      });
      return true;
    });
    assert.equal(attempts, 1);
    assert.deepEqual(delays, []);
  } finally {
    rmSync(root, { recursive: true });
  }
});
test("cleanup revalidation failures retain filesystem diagnostics without removing roots", async () => {
  const root = mkdtempSync(path.join(process.cwd(), ".nc1-cleanup-test-"));
  try {
    const roots = freshRoots(root);
    await assert.rejects(
      cleanupRoots({ ...roots, proof: path.join(root, "missing-proof") }),
      (error) => {
        assert.equal(error.message, "roots-cleanup-failed");
        assert.deepEqual(error.cleanupError, {
          code: "ENOENT",
          syscall: "lstat",
          path: "owned-root",
          attempts: 1,
        });
        assert.equal(error.cause.code, "ENOENT");
        return true;
      },
    );
    assert.equal(existsSync(roots.profile), true);
    assert.equal(existsSync(roots.project), true);
  } finally {
    rmSync(root, { recursive: true });
  }
});
test("later profile cleanup diagnostics survive earlier teardown and capture blockers", async () => {
  const { retainCaptureError } =
    await import("./capture-native-chat-drive.mjs");
  const timing = captureTiming();
  const teardown = new Error("native-tool-blocked");
  const cleanupError = {
    code: "EPERM",
    syscall: "rmdir",
    path: "profile/EBWebView",
    attempts: 21,
  };
  const roots = Object.assign(new Error("roots-cleanup-failed"), {
    cleanupError,
  });
  const failure = await cleanupCapture(timing.record, [
    {
      step: "vite-stop",
      action: async () => {
        throw teardown;
      },
    },
    {
      step: "roots-cleanup",
      action: async () => {
        throw roots;
      },
    },
  ]);
  assert.equal(failure, teardown);
  assert.deepEqual(failure.cleanupError, cleanupError);
  const primary = new Error("state-timeout");
  assert.equal(retainCaptureError(primary, failure), primary);
  let probe;
  const result = persistCapture(
    {
      output: { probe: "blocked.probe.json" },
      options: parseArgs(captureArgs),
      run,
      directory: "ignored",
      timing,
      elapsedMs: 100,
      error: primary,
    },
    (_file, bytes) => {
      probe = JSON.parse(bytes);
    },
  );
  assert.equal(result.reason, "state-timeout");
  assert.deepEqual(probe.cleanupError, cleanupError);
});
test("post-cleanup PNG write failure still saves a blocked timestamped probe", () => {
  const writes = [];
  const timing = {
    steps: [],
    record(step) {
      this.steps.push({ step, atMs: 100 });
    },
  };
  const root = mkdtempSync(path.join(process.cwd(), ".nc1-promote-test-"));
  try {
    const { options, files, png } = stagedPair(root);
    const probe = JSON.parse(readFileSync(files.probe, "utf8"));
    unlinkSync(files.png);
    const result = persistCapture(
      {
        output: files,
        options,
        run,
        directory: "ignored",
        timing,
        elapsedMs: 100,
        captured: { screenshot: png, probe },
      },
      (file, bytes) => {
        writes.push(typeof file === "number" ? files.png : file);
        if (typeof file === "number") throw new Error("write-denied");
        const blocked = JSON.parse(bytes);
        assert.equal(blocked.verdict, "blocked");
        assert.equal(blocked.timing.steps.at(-1).step, "capture-blocked");
      },
    );
    assert.deepEqual(writes, [files.png, files.probe]);
    assert.equal(result.verdict, "blocked");
    assert.equal(result.reason, "write-denied");
  } finally {
    rmSync(root, { recursive: true });
  }
});
test("partial PNGs and PNGs whose success probe cannot be saved are rolled back", () => {
  for (const partial of [true, false]) {
    const root = mkdtempSync(path.join(process.cwd(), ".nc1-promote-test-"));
    try {
      const { options, files, png } = stagedPair(root);
      const probe = JSON.parse(readFileSync(files.probe, "utf8"));
      unlinkSync(files.png);
      const timing = {
        steps: [],
        record(step) {
          this.steps.push({ step, atMs: 100 });
        },
      };
      const result = persistCapture(
        {
          output: files,
          options,
          run,
          directory: "ignored",
          timing,
          elapsedMs: 100,
          captured: { screenshot: png, probe },
        },
        (file, bytes) => {
          if (partial && (typeof file === "number" || file === files.png)) {
            writeFileSync(file, Buffer.alloc(8));
            throw new Error("write-denied");
          }
          if (
            !partial &&
            file === files.probe &&
            JSON.parse(bytes).verdict === "captured"
          )
            throw new Error("write-denied");
          writeFileSync(file, bytes);
        },
      );
      assert.equal(result.verdict, "blocked");
      assert.equal(existsSync(files.png), false);
      assert.equal(
        JSON.parse(readFileSync(files.probe, "utf8")).verdict,
        "blocked",
      );
    } finally {
      rmSync(root, { recursive: true });
    }
  }
});
test("capture persistence never replaces or rolls back a preexisting PNG", () => {
  const root = mkdtempSync(path.join(process.cwd(), ".nc1-promote-test-"));
  try {
    const { options, files, png } = stagedPair(root);
    const probe = JSON.parse(readFileSync(files.probe, "utf8"));
    const timing = {
      steps: [],
      record(step) {
        this.steps.push({ step, atMs: 100 });
      },
    };
    const result = persistCapture({
      output: files,
      options,
      run,
      directory: "ignored",
      timing,
      elapsedMs: 100,
      captured: { screenshot: png, probe },
    });
    assert.equal(result.verdict, "blocked");
    assert.deepEqual(readFileSync(files.png), png);
  } finally {
    rmSync(root, { recursive: true });
  }
});
test("tracked index flags stream across chunks, preserve unusual paths and fail closed", async () => {
  const parser = trackedIndexParser();
  for (const part of ["H scripts/cap", "ture\nS not-a-tag.mjs\0H another\0"])
    parser.feed(part);
  parser.finish();
  const exact = trackedIndexParser();
  exact.feed("H x\0".repeat(200000));
  exact.finish();
  assert.throws(() => exact.feed("H y\0"), /source-index-bound/u);
  for (const malformed of [
    "H truncated",
    "? invalid\0",
    "H " + "x".repeat(4095) + "\0",
  ])
    assert.throws(() => {
      const invalid = trackedIndexParser();
      invalid.feed(malformed);
      invalid.finish();
    }, /source-index-(record|bound)/u);
  for (const tag of ["S", "h", "s", "m"]) {
    const hidden = trackedIndexParser();
    hidden.feed(tag);
    assert.throws(
      () => hidden.feed(" scripts/capture-native-chat.mjs\0"),
      /source-index-flags/u,
    );
  }
  const fake = child();
  fake.kill = () => {
    fake.killed = true;
  };
  const pending = readTrackedIndex(
    "git",
    ["-C", workspace, "ls-files", "-v", "-z"],
    () => fake,
  );
  fake.stdout.emit("data", Buffer.from("H safe\0S hidden\0"));
  await assert.rejects(pending, /source-index-flags/u);
  assert.equal(fake.killed, true);
  const streamed = child();
  streamed.kill = () => {
    throw new Error("unexpected-index-kill");
  };
  const accepted = readTrackedIndex(
    "git",
    ["-C", workspace, "ls-files", "-v", "-z"],
    () => streamed,
  );
  streamed.stdout.emit("data", Buffer.from("H safe\0".repeat(20000)));
  streamed.emit("close", 0);
  assert.equal(await accepted, "");
  for (const failure of ["error", "close"]) {
    const failed = child();
    failed.kill = () => {};
    const pendingFailure = readTrackedIndex("git", [], () => failed);
    failed.emit(failure, 1);
    await assert.rejects(pendingFailure, /source-index-tool/u);
  }
});
test("capture requires clean source from its exact checkout and stable HEAD", async () => {
  const tool =
    (
      status = "",
      head = source,
      root = workspace,
      flags = "H scripts/capture-native-chat.mjs\0",
    ) =>
    async (_, args) => {
      assert.deepEqual(args.slice(0, 2), ["-C", workspace]);
      if (args[2] === "ls-files") {
        assert.deepEqual(args.slice(3), ["-v", "-z"]);
        return flags;
      }
      if (args[2] === "status") {
        assert.deepEqual(args.slice(3), [
          "--porcelain=v1",
          "--untracked-files=all",
        ]);
        return status;
      }
      return args[3] === "--show-toplevel" ? root : head;
    };
  assert.equal(await cleanSource(workspace, tool()), source);
  for (const flag of ["S", "h", "s", "m"])
    await assert.rejects(
      cleanSource(
        workspace,
        tool(
          "",
          source,
          workspace,
          `${flag} scripts/capture-native-chat.mjs\0`,
        ),
      ),
      /source-index-flags/u,
    );
  await assert.rejects(
    verifySource(
      workspace,
      source,
      tool("", source, workspace, "S scripts/capture-native-chat.mjs\0"),
    ),
    /source-index-flags/u,
  );
  for (const status of [" M src/main.tsx", "?? src/new.ts", "A  src/main.tsx"])
    await assert.rejects(cleanSource(workspace, tool(status)), /source-dirty/u);
  await assert.rejects(
    cleanSource(workspace, tool("", "wrong")),
    /source-identity/u,
  );
  await assert.rejects(
    cleanSource(workspace, tool("", source, path.resolve("other"))),
    /source-root/u,
  );
  await assert.rejects(
    verifySource(workspace, source, tool("", "b".repeat(40))),
    /source-drift/u,
  );
});
test("owned Vite launch pins checkout root, config, mode and strict port", () => {
  const launch = viteLaunch(workspace);
  assert.equal(launch.executable, process.execPath);
  assert.deepEqual(launch.args, [
    path.join(workspace, "node_modules/vite/bin/vite.js"),
    workspace,
    "--config",
    path.join(workspace, "vite.config.ts"),
    "--host",
    "127.0.0.1",
    "--port",
    "1420",
    "--strictPort",
    "--mode",
    "desktop-native",
    "--clearScreen",
    "false",
  ]);
  assert.equal(launch.options.cwd, workspace);
  assert.equal(launch.options.windowsHide, true);
});
const child = () =>
  Object.assign(new EventEmitter(), {
    pid: 1,
    exitCode: null,
    signalCode: null,
    stdout: new EventEmitter(),
    stderr: new EventEmitter(),
  });
test("readiness belongs to owned stdout; timeout, early exit and bounds fail closed", async () => {
  const ready = child();
  const waiting = waitForVite(ready, 1000);
  ready.stdout.emit("data", "Local: http://127.0.0.1:1420/");
  await waiting;
  for (const trigger of [
    (candidate) => candidate.emit("exit", 1),
    (candidate) => candidate.emit("error", new Error("private")),
    (candidate) => candidate.stdout.emit("data", Buffer.alloc(65537)),
    (candidate) => candidate.stderr.emit("data", "http://127.0.0.1:1420/"),
  ]) {
    const candidate = child();
    const pending = waitForVite(candidate, 5);
    trigger(candidate);
    await assert.rejects(
      pending,
      /owned-vite-(exited|blocked|output-bound|timeout)/u,
    );
  }
});
test("served asset hashes exact bounded body and rejects status and oversized responses", async () => {
  const body = "export const synthetic = 1;";
  const fetcher = async (url, options) => {
    assert.equal(url, "http://127.0.0.1:1420/src/main.tsx");
    assert.ok(options.signal);
    return new Response(body);
  };
  assert.equal(
    await servedAssetHash(fetcher),
    createHash("sha256").update(body).digest("hex"),
  );
  const raw = Buffer.from([0xef, 0xbb, 0xbf, 0xff]);
  assert.equal(
    await servedAssetHash(async () => new Response(raw)),
    createHash("sha256").update(raw).digest("hex"),
  );
  await assert.rejects(
    servedAssetHash(async () => new Response(body, { status: 404 })),
    /served-asset-status/u,
  );
  await assert.rejects(
    servedAssetHash(async () => new Response("x".repeat(65537))),
    /cdp-discovery-bound/u,
  );
});

test("cold served-asset reads get a bounded longer wait and time out explicitly", async () => {
  let observed;
  const slow = async (_, options) =>
    new Promise((_resolve, reject) => {
      observed = options.signal;
      options.signal.addEventListener("abort", () =>
        reject(options.signal.reason),
      );
    });
  // AbortSignal.timeout timers are unreferenced; hold the loop open here.
  const keepAlive = setInterval(() => {}, 1000);
  try {
    await assert.rejects(
      servedAssetHash(slow, 20),
      /^Error: served-asset-timeout$/u,
    );
  } finally {
    clearInterval(keepAlive);
  }
  assert.ok(observed.aborted);
  assert.equal(COLD_ASSET_TIMEOUT_MS, 60000);
  for (const timeout of [0, 60001, 1.5, Number.NaN])
    await assert.rejects(
      servedAssetHash(async () => new Response("x"), timeout),
      /served-asset-timeout/u,
    );
  assert.equal(
    await servedAssetHash(async () => new Response("x"), COLD_ASSET_TIMEOUT_MS),
    createHash("sha256").update("x").digest("hex"),
  );
});
test("named build checkout admission rejects untracked native inputs", async () => {
  const build = path.resolve("synthetic-build");
  await assert.rejects(
    buildCheckout(
      workspace,
      build,
      async (_, args) => {
        if (args[2] === "ls-files") return "H src-tauri/Cargo.toml\0";
        if (args[2] === "status")
          return args.includes("--untracked-files=all")
            ? "?? src-tauri/src/untracked.rs"
            : "";
        return "e".repeat(40);
      },
      () => {},
    ),
    /build-checkout-dirty/u,
  );
});
test("a named build checkout must be absolute, clean and carry the same native tree", async () => {
  const build = path.resolve("synthetic-build");
  const tree = "e".repeat(40);
  const plain = [];
  const tool =
    ({ workspaceTree = tree, buildTree = tree, status = "" } = {}) =>
    async (_, args) => {
      if (args[2] === "ls-files") return "H src-tauri/Cargo.toml\0";
      if (args[2] === "status") {
        assert.deepEqual(args, [
          "-C",
          build,
          "status",
          "--porcelain=v1",
          "--untracked-files=all",
          "--",
          "src-tauri",
        ]);
        return status;
      }
      assert.deepEqual(args.slice(2), ["rev-parse", "HEAD:src-tauri"]);
      return args[1] === workspace ? workspaceTree : buildTree;
    };
  const check = (file, directory = false) => plain.push([file, directory]);
  assert.equal(
    await buildCheckout(workspace, undefined, tool(), check),
    workspace,
  );
  assert.equal(await buildCheckout(workspace, "", tool(), check), workspace);
  assert.equal(plain.length, 0);
  assert.equal(await buildCheckout(workspace, build, tool(), check), build);
  assert.deepEqual(plain, [
    [build, true],
    [path.join(build, "src-tauri", "Cargo.toml"), false],
  ]);
  await assert.rejects(
    buildCheckout(workspace, "relative", tool(), check),
    /^Error: build-checkout$/u,
  );
  await assert.rejects(
    buildCheckout(workspace, build, tool({ buildTree: "f".repeat(40) }), check),
    /build-checkout-mismatch/u,
  );
  await assert.rejects(
    buildCheckout(
      workspace,
      build,
      tool({ workspaceTree: "bad", buildTree: "bad" }),
      check,
    ),
    /build-checkout-mismatch/u,
  );
  await assert.rejects(
    buildCheckout(
      workspace,
      build,
      tool({ status: " M src-tauri/src/lib.rs" }),
      check,
    ),
    /build-checkout-dirty/u,
  );
  await assert.rejects(
    buildCheckout(workspace, build, tool(), () => {
      throw new Error("path-reparse");
    }),
    /path-reparse/u,
  );
});

const run = "12345678-1234-1234-1234-123456789abc";
const captureArgs = [
  "--harness",
  "claude",
  "--state",
  "turn-completed",
  "--width",
  "480",
];
test("staging is ignored and run identifiers cannot escape its directory", () => {
  const files = stagingPaths(workspace, run, parseArgs(captureArgs));
  assert.equal(
    files.directory,
    path.join(workspace, "work/native-captures", run),
  );
  assert.equal(
    files.png,
    path.join(files.directory, "turn-completed-claude-480.png"),
  );
  assert.match(
    readFileSync(new URL("../.gitignore", import.meta.url), "utf8"),
    /^work\/$/mu,
  );
  for (const invalid of [
    "../private",
    "..\\private",
    run + "/more",
    "C:private",
    "other",
  ])
    assert.throws(
      () => stagingPaths(workspace, invalid, parseArgs(captureArgs)),
      /capture-run/u,
    );
});
test("promotion requires explicit reviewed acknowledgement and closed arguments", () => {
  const args = ["--run", run, ...captureArgs, "--reviewed"];
  assert.deepEqual(parsePromotionArgs(args), {
    ...parseArgs(captureArgs),
    run,
    reviewed: true,
  });
  for (const invalid of [
    args.slice(0, -1),
    [...args, "--reviewed"],
    args.map((value) => (value === run ? "../private" : value)),
  ])
    assert.throws(() => parsePromotionArgs(invalid));
});
function stagedPair(root, mutate = () => {}) {
  const options = parsePromotionArgs([
    "--run",
    run,
    ...captureArgs,
    "--reviewed",
  ]);
  const files = stagingPaths(root, run, options);
  mkdirSync(files.directory, { recursive: true });
  const probe = {
    schemaVersion: 1,
    caseId: options.caseId,
    bindingId: "claude-agent-acp",
    turnId: "turn-synthetic",
    hashes: {
      build: "a".repeat(64),
      fixture: "b".repeat(64),
      source: "c".repeat(40),
      servedAsset: "d".repeat(64),
    },
    protocol: "standard-acp-1",
    webviewVersion: "130.0.1.0",
    dpr: 1,
    dimensions: {
      outer: { width: 496, height: 1039 },
      client: { width: 480, height: 1000 },
      css: { width: 480, height: 1000 },
    },
    predicates: { namedState: true, binding: true },
    elements: [],
    controls: { send: true },
    overflow: { horizontal: false },
    timing: { elapsedMs: 100, steps: [] },
    verdict: "captured",
  };
  const png = Buffer.alloc(24);
  Buffer.from("89504e470d0a1a0a", "hex").copy(png);
  png.write("IHDR", 12);
  png.writeUInt32BE(480, 16);
  png.writeUInt32BE(1000, 20);
  mutate(probe);
  writeFileSync(files.png, png);
  writeFileSync(files.probe, JSON.stringify(probe));
  return { options, files, png };
}
test("promotion copies only a validated reviewed pair and never overwrites evidence", () => {
  const root = mkdtempSync(path.join(process.cwd(), ".nc1-promote-test-"));
  try {
    const { options, files, png } = stagedPair(root);
    assert.throws(
      () => promoteCapture(root, { ...options, reviewed: false }),
      /review-required/u,
    );
    const result = promoteCapture(root, options);
    const destination = path.join(
      root,
      "docs/work-items/CODEX-PARITY/evidence",
      path.basename(files.png),
    );
    assert.deepEqual(readFileSync(destination), png);
    assert.equal(result.verdict, "promoted");
    assert.equal(existsSync(files.png), true);
    assert.throws(() => promoteCapture(root, options), /evidence-exists/u);
  } finally {
    rmSync(root, { recursive: true });
  }
});
test("promotion rejects blocked, private, mismatched and oversized pairs before copying", () => {
  for (const mutate of [
    (probe) => (probe.verdict = "blocked"),
    (probe) =>
      (probe.elements = [
        {
          kind: "tool",
          text: "sk-ant-private",
          x: 0,
          y: 0,
          width: 1,
          height: 1,
        },
      ]),
    (probe) => (probe.caseId = "claude-tool-running"),
    (probe) => (probe.dimensions.client.width = 720),
    (probe) => (probe.extra = "x".repeat(65537)),
  ]) {
    const root = mkdtempSync(path.join(process.cwd(), ".nc1-promote-test-"));
    try {
      const { options } = stagedPair(root, mutate);
      assert.throws(() => promoteCapture(root, options));
      assert.equal(
        existsSync(path.join(root, "docs/work-items/CODEX-PARITY/evidence")),
        false,
      );
    } finally {
      rmSync(root, { recursive: true });
    }
  }
});

test("promotion rejects private PNG metadata, viewport mismatch and oversized PNGs", () => {
  for (const transform of [
    (png) => Buffer.concat([png, Buffer.from("sk-ant-private")]),
    (png) => {
      const wrong = Buffer.from(png);
      wrong.writeUInt32BE(464, 16);
      return wrong;
    },
    () => Buffer.alloc(8 * 1024 * 1024 + 1),
  ]) {
    const root = mkdtempSync(path.join(process.cwd(), ".nc1-promote-test-"));
    try {
      const { options, files, png } = stagedPair(root);
      writeFileSync(files.png, transform(png));
      assert.throws(() => promoteCapture(root, options));
      assert.equal(
        existsSync(path.join(root, "docs/work-items/CODEX-PARITY/evidence")),
        false,
      );
    } finally {
      rmSync(root, { recursive: true });
    }
  }
});

test("promotion privacy checks raw values hidden by duplicate JSON keys", () => {
  const root = mkdtempSync(path.join(process.cwd(), ".nc1-promote-test-"));
  try {
    const { options, files } = stagedPair(root);
    const clean = readFileSync(files.probe, "utf8");
    writeFileSync(
      files.probe,
      '{"elements":[{"text":"sk-ant-private"}],' + clean.slice(1),
    );
    assert.throws(() => promoteCapture(root, options), /privacy/u);
    assert.equal(
      existsSync(path.join(root, "docs/work-items/CODEX-PARITY/evidence")),
      false,
    );
  } finally {
    rmSync(root, { recursive: true });
  }
});
