import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { MAX_TRANSCRIPT_BYTES } from "./limits.mjs";
import {
  diffShapes,
  diffTiming,
  shapeOfHarness,
  shapeOfRecords,
} from "./shape.mjs";

const corpus = path.join("src-tauri", "tests", "fixtures", "harness-wire");
const MAX_HARNESS_BYTES = 640 * 1024;
const harnesses = fs
  .readdirSync(corpus, { withFileTypes: true })
  .filter((entry) => entry.isDirectory())
  .map((entry) => entry.name);

test("the corpus covers Claude Code, Codex and Pi", () => {
  assert.deepEqual([...harnesses].sort(), ["claude", "codex", "pi"]);
});

for (const harness of harnesses) {
  const dir = path.join(corpus, harness);
  const files = fs.readdirSync(dir);
  const meta = JSON.parse(fs.readFileSync(path.join(dir, "meta.json"), "utf8"));
  const transcripts = files
    .filter((name) => name.endsWith(".jsonl"))
    .map((name) => name.slice(0, -6));

  test(`${harness}: shape.json is the shape of the committed transcripts`, () => {
    const committed = JSON.parse(
      fs.readFileSync(path.join(dir, "shape.json"), "utf8"),
    );
    assert.deepEqual(
      committed,
      shapeOfHarness(dir),
      "run shape.mjs --write and review the diff",
    );
  });

  test(`${harness}: transcripts stay bounded`, () => {
    let total = 0;
    for (const name of files) {
      const bytes = fs.statSync(path.join(dir, name)).size;
      total += bytes;
      if (name.endsWith(".jsonl"))
        assert.ok(bytes <= MAX_TRANSCRIPT_BYTES, `${name} is ${bytes} bytes`);
    }
    assert.ok(
      total <= MAX_HARNESS_BYTES,
      `${harness} corpus is ${total} bytes`,
    );
  });

  test(`${harness}: transcripts carry no email, secret or process id`, () => {
    for (const name of files) {
      const body = fs.readFileSync(path.join(dir, name), "utf8");
      for (const email of body.match(
        /[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/g,
      ) ?? []) {
        assert.ok(email.endsWith("@example.invalid"), `${name} holds ${email}`);
      }
      assert.doesNotMatch(body, /sk-ant-|"pid":[1-9]/, name);
      // A home folder, even split across streamed pieces.
      assert.doesNotMatch(
        body,
        /\\\\Users\\\\|\/Users\/|\/home\/(?!fixture)/i,
        name,
      );
    }
  });

  test(`${harness}: every transcript is replayed and every scenario is accounted for`, () => {
    for (const scenario of transcripts) {
      assert.ok(
        files.includes(`${scenario}.replay.json`),
        `${scenario} has no committed replay`,
      );
      assert.ok(
        meta.scenarios[scenario],
        `${scenario} is missing from meta.json`,
      );
    }
    for (const scenario of Object.keys(meta.scenarios)) {
      assert.ok(
        transcripts.includes(scenario),
        `meta.json names ${scenario} with no transcript`,
      );
    }
    for (const [scenario, reason] of Object.entries(meta.gaps ?? {})) {
      assert.ok(
        !transcripts.includes(scenario),
        `${scenario} is both captured and a gap`,
      );
      assert.ok(reason.length > 0);
    }
  });
}

const frames = (frame) => [
  {
    direction: "out",
    frame: {
      type: "control_request",
      request_id: "sage-1",
      request: { subtype: "initialize" },
    },
  },
  { direction: "in", frame },
];
const shape = (records) => {
  const raw = shapeOfRecords(records);
  return {
    version: "1",
    commands: [...raw.commands].sort(),
    kinds: Object.fromEntries(
      Object.entries(raw.kinds).map(([kind, set]) => [kind, [...set].sort()]),
    ),
  };
};

test("a new field on a known event is drift (Claude Code 2.1.330 is_backgrounded)", () => {
  const before = shape(
    frames({ type: "system", subtype: "task_started", task_id: "t" }),
  );
  const after = shape(
    frames({
      type: "system",
      subtype: "task_started",
      task_id: "t",
      is_backgrounded: false,
    }),
  );
  assert.deepEqual(diffShapes(before, after), [
    "added field system/task_started is_backgrounded",
  ]);
});

test("a removed slash command and a new event type are drift", () => {
  const reply = (commands) => ({
    type: "control_response",
    response: {
      subtype: "success",
      request_id: "sage-1",
      response: { commands },
    },
  });
  const before = shape(
    frames(reply([{ name: "review" }, { name: "pr-comments" }])),
  );
  const after = shape([
    ...frames(reply([{ name: "review" }])),
    {
      direction: "in",
      frame: {
        type: "rate_limit_event",
        rate_limit_info: { status: "allowed" },
      },
    },
  ]);
  assert.deepEqual(diffShapes(before, after), [
    "removed command commands:pr-comments",
    "added event rate_limit_event",
  ]);
});

test("JSON-RPC replies are named after the request they answer", () => {
  const raw = shapeOfRecords([
    { direction: "out", frame: { id: 3, method: "skills/list" } },
    {
      direction: "in",
      frame: { id: 3, result: { data: [{ skills: [{ name: "imagegen" }] }] } },
    },
    {
      direction: "in",
      frame: {
        method: "item/started",
        params: { item: { type: "commandExecution" } },
      },
    },
  ]);
  assert.deepEqual(Object.keys(raw.kinds).sort(), [
    "notify:item/started:commandExecution",
    "result:skills/list",
  ]);
  assert.deepEqual([...raw.commands], ["skills:imagegen"]);
});

test("a start that more than doubles by over two seconds is drift", () => {
  const meta = (ms) => ({
    scenarios: { shell: { outcome: "completed", firstFrameMs: ms } },
  });
  assert.deepEqual(diffTiming(meta(1500), meta(2900)), []);
  assert.deepEqual(diffTiming(meta(1500), meta(9000)), [
    "slower start shell: first frame 1500ms -> 9000ms",
  ]);
});
