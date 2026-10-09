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
} from "node:fs";
import { parseArgs } from "./capture-native-chat-contract.mjs";
import {
  stagingPaths,
  parsePromotionArgs,
  promoteCapture,
} from "./capture-native-chat-evidence.mjs";
import {
  cleanSource,
  verifySource,
  viteLaunch,
  waitForVite,
  servedAssetHash,
} from "./capture-native-chat-source.mjs";

const workspace = path.resolve("synthetic-checkout");
const source = "a".repeat(40);
test("capture requires clean source from its exact checkout and stable HEAD", async () => {
  const tool =
    (status = "", head = source, root = workspace) =>
    async (_, args) => {
      assert.deepEqual(args.slice(0, 2), ["-C", workspace]);
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
      outer: { width: 480, height: 1000 },
      client: { width: 464, height: 961 },
      css: { width: 464, height: 961 },
    },
    predicates: { namedState: true, binding: true },
    elements: [],
    controls: { send: true },
    overflow: { horizontal: false },
    timing: { elapsedMs: 100 },
    verdict: "captured",
  };
  const png = Buffer.alloc(24);
  Buffer.from("89504e470d0a1a0a", "hex").copy(png);
  png.write("IHDR", 12);
  png.writeUInt32BE(464, 16);
  png.writeUInt32BE(961, 20);
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
    (probe) => (probe.dimensions.outer.width = 720),
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
      wrong.writeUInt32BE(480, 16);
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
