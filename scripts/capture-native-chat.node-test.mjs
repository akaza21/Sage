import assert from "node:assert/strict";
import test from "node:test";
import path from "node:path";
import {
  mkdtempSync,
  mkdirSync,
  symlinkSync,
  existsSync,
  rmSync,
} from "node:fs";
import { ensurePlainDirectory } from "./capture-native-chat-process.mjs";
import {
  parseArgs,
  evidencePaths,
  validateProbe,
  admitTarget,
  validateDimensions,
  validatePng,
  waitForState,
  withOwnedCapture,
  readBoundedResponse,
} from "./capture-native-chat-contract.mjs";

import {
  namedState,
  visibleRectangle,
  actionsFor,
} from "./capture-native-chat-dom.mjs";

const args = [
  "--harness",
  "claude",
  "--state",
  "turn-completed",
  "--width",
  "480",
];
test("capture arguments are closed, finite and reject duplicates", () => {
  assert.deepEqual(parseArgs(args), {
    harness: "claude",
    state: "turn-completed",
    width: 480,
    height: 1000,
    caseId: "claude-turn-completed",
  });
  for (const bad of [
    [],
    [...args, "--width", "720"],
    [...args, "--port", "1420"],
    args.map((value) => (value === "480" ? "481" : value)),
    args.map((value) => (value === "turn-completed" ? "../private" : value)),
  ])
    assert.throws(() => parseArgs(bad));
});
test("evidence paths cannot escape the selected directory", () => {
  const root = path.resolve("synthetic-evidence");
  assert.equal(
    evidencePaths(root, parseArgs(args)).png,
    path.join(root, "turn-completed-claude-480.png"),
  );
  for (const state of [
    "../private",
    "..\\private",
    "/private",
    "turn-completed:secret",
  ])
    assert.throws(() => evidencePaths(root, { ...parseArgs(args), state }));
});
test("capture explicitly rejects truthful-absent pairs", () => {
  for (const [harness, state] of [
    ["codex", "compaction-started"],
    ["codex", "compaction-failed"],
    ["codex", "compaction-cancelled"],
    ["claude", "effort-menu"],
    ["claude", "context-meter"],
    ["claude", "child-opened"],
  ])
    assert.throws(
      () =>
        parseArgs(["--harness", harness, "--state", state, "--width", "480"]),
      /truthful-absent-state/u,
    );
});
const probe = () => ({
  schemaVersion: 1,
  caseId: "claude-turn-completed",
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
  controls: { model: true },
  overflow: { horizontal: false },
  timing: { elapsedMs: 100 },
  verdict: "captured",
});
test("probe exact limits pass; plus-one, privacy and unknown fields fail", () => {
  const exact = probe();
  exact.elements = Array.from({ length: 64 }, () => ({
    kind: "tool",
    text: "x".repeat(512),
    x: 0,
    y: 0,
    width: 10,
    height: 20,
  }));
  assert.equal(validateProbe(exact), exact);
  for (const mutate of [
    (value) => value.elements.push(value.elements[0]),
    (value) => (value.elements[0].text += "x"),
    (value) => (value.elements[0].text = "sk-ant-private"),
    (value) => (value.dpr = 1.25),
    (value) => (value.bindingId = "codex-app-server"),
    (value) => (value.protocol = "codex-app-server-0.146.0"),
    (value) => (value.dump = "private"),
    (value) => (value.hashes.servedAsset = "d".repeat(63)),
    (value) => (value.caseId = "claude-effort-menu"),
    (value) => (value.elements[0].x = Infinity),
    (value) => (value.elements[0].text = "C:\\Users\\someone\\secret"),
  ]) {
    const value = structuredClone(exact);
    mutate(value);
    assert.throws(() => validateProbe(value));
  }
});
test("DPR, clamp, wrong CDP target and PNG viewport mismatches fail closed", () => {
  const value = probe();
  assert.doesNotThrow(() => validateDimensions(value.dimensions, 480, 1000, 1));
  assert.throws(() => validateDimensions(value.dimensions, 720, 1000, 1));
  assert.throws(() => validateDimensions(value.dimensions, 480, 1000, 2));
  const target = {
    id: "owned",
    type: "page",
    title: "Sage",
    url: "http://127.0.0.1:1420/",
    webSocketDebuggerUrl: "ws://127.0.0.1:9223/devtools/page/owned",
  };
  assert.equal(admitTarget([target], 9223), target);
  assert.throws(() => admitTarget([target, target], 9223));
  assert.throws(() =>
    admitTarget([{ ...target, url: "https://example.invalid/" }], 9223),
  );
  const png = Buffer.alloc(24);
  Buffer.from("89504e470d0a1a0a", "hex").copy(png);
  png.write("IHDR", 12);
  png.writeUInt32BE(464, 16);
  png.writeUInt32BE(961, 20);
  assert.doesNotThrow(() => validatePng(png, value.dimensions.css));
  assert.throws(() => validatePng(png, { width: 480, height: 1000 }));
});
test("stalled predicates are bounded and teardown runs on failure", async () => {
  await assert.rejects(
    waitForState(async () => false, Date.now() + 5),
    /state-timeout/u,
  );
  const actions = [];
  await assert.rejects(
    withOwnedCapture(
      async () => {
        throw Error("blocked");
      },
      async () => actions.push("cdp"),
      async () => actions.push("owned-process"),
    ),
    /blocked/u,
  );
  assert.deepEqual(actions, ["cdp", "owned-process"]);
});

test("named states do not mistake settled compaction or allowed approvals", () => {
  const observation = {
    bindingId: "codex-app-server",
    selected: "codex",
    state: "running",
    visible: { compaction: true, permission: true },
    itemKinds: [],
    controls: {},
    compactionStates: ["completed"],
    permissionStates: ["resolvedAllowed"],
  };
  const options = { harness: "codex", state: "compaction-started" };
  assert.equal(namedState(options, observation).namedState, false);
  assert.equal(
    namedState(
      { ...options, state: "compaction-cancelled" },
      { ...observation, state: "cancelled" },
    ).namedState,
    false,
  );
  assert.equal(
    namedState(
      { harness: "claude", state: "approval-declined" },
      { ...observation, state: "completed" },
    ).namedState,
    false,
  );
  assert.equal(
    namedState(
      { ...options, state: "compaction-completed" },
      { ...observation, state: "completed" },
    ).namedState,
    true,
  );
  const wrongBinding = namedState(options, {
    ...observation,
    bindingId: "foreign",
    compactionStates: ["started"],
  });
  assert.equal(wrongBinding.binding, false);
  assert.equal(
    namedState(options, {
      ...observation,
      visible: {},
      compactionStates: ["started"],
    }).namedState,
    false,
  );
});

test("only rendered rectangles inside the viewport can prove visible state", () => {
  const viewport = { width: 480, height: 1000 };
  assert.equal(
    visibleRectangle({ x: 0, y: 0, width: 20, height: 20 }, viewport),
    true,
  );
  assert.equal(
    visibleRectangle({ x: 480, y: 0, width: 20, height: 20 }, viewport),
    false,
  );
  assert.equal(
    visibleRectangle({ x: 0, y: -20, width: 20, height: 20 }, viewport),
    false,
  );
  assert.equal(
    visibleRectangle({ x: 0, y: 0, width: 0, height: 20 }, viewport),
    false,
  );
  assert.equal(
    visibleRectangle({ x: NaN, y: 0, width: 20, height: 20 }, viewport),
    false,
  );
});
test("CDP discovery body exact bound passes; plus-one cancels the stream", async () => {
  let cancelled = false;
  const response = (length) => ({
    body: new ReadableStream({
      start(controller) {
        controller.enqueue(new Uint8Array(length).fill(32));
        if (length <= 65536) controller.close();
      },
      cancel() {
        cancelled = true;
      },
    }),
  });
  assert.equal((await readBoundedResponse(response(65536))).length, 65536);
  await assert.rejects(
    readBoundedResponse(response(65537)),
    /cdp-discovery-bound/u,
  );
  assert.equal(cancelled, true);
});

test("directory creation rejects linked ancestors before creating descendants", (context) => {
  const root = mkdtempSync(path.join(process.cwd(), ".nc1-path-test-"));
  try {
    const target = path.join(root, "target"),
      link = path.join(root, "link");
    mkdirSync(target);
    try {
      symlinkSync(
        target,
        link,
        process.platform === "win32" ? "junction" : "dir",
      );
    } catch (error) {
      if (["EPERM", "EACCES", "ENOSYS"].includes(error.code)) {
        context.skip("links unavailable");
        return;
      }
      throw error;
    }
    assert.throws(
      () => ensurePlainDirectory(path.join(link, "uncreated")),
      /path-reparse/u,
    );
    assert.equal(existsSync(path.join(target, "uncreated")), false);
    assert.equal(
      ensurePlainDirectory(path.join(root, "plain", "nested")),
      path.join(root, "plain", "nested"),
    );
  } finally {
    rmSync(root, { recursive: true });
  }
});

test("plan entry requires an offered plan mode, not a permission control", () => {
  const observation = {
    state: "completed",
    visible: { addMenu: true, access: true },
    itemKinds: [],
    controls: {},
  };
  assert.equal(
    namedState({ harness: "claude", state: "plan-mode-entry" }, observation)
      .namedState,
    false,
  );
  assert.deepEqual(actionsFor("approval-approved"), [
    ".acp-permission__actions button:first-child",
    ".acp-turn:last-of-type .chat-transcript-work > summary",
  ]);
});
