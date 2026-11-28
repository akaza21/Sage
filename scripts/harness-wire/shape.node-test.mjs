import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { MAX_TRANSCRIPT_BYTES } from "./limits.mjs";
import { makeSanitizer } from "./sanitize.mjs";
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

test("an account's model catalog is redacted where a model is named, and only there", () => {
  const sanitizer = makeSanitizer({ roots: [] });
  const [pi, codex, turn] = sanitizer.records([
    {
      direction: "in",
      frame: {
        type: "response",
        command: "get_available_models",
        data: {
          models: [
            {
              id: "private-x",
              name: "Private X",
              api: "messages",
              provider: "acme",
            },
            // A custom model whose id is also a content-block type.
            { id: "text", name: "Text", api: "messages", provider: "acme" },
          ],
        },
      },
    },
    {
      direction: "in",
      frame: {
        id: 2,
        result: {
          data: [
            { id: "private-y", model: "private-y", displayName: "Private Y" },
          ],
        },
      },
    },
    {
      direction: "in",
      frame: {
        type: "message_end",
        message: {
          model: "private-x",
          content: [{ type: "text", text: "text" }],
        },
      },
    },
  ]);
  assert.deepEqual(
    pi.frame.data.models.map(({ id, name }) => [id, name]),
    [
      ["model-1", "Model 1"],
      ["model-2", "Model 2"],
    ],
  );
  assert.deepEqual(codex.frame.result.data[0], {
    id: "model-3",
    model: "model-3",
    displayName: "redacted",
  });
  assert.deepEqual(turn.frame.message, {
    model: "model-1",
    content: [{ type: "text", text: "text" }],
  });
  assert.doesNotMatch(JSON.stringify([pi, codex, turn]), /private|Private/);
});

test("the account's usage and the provider's request ids are normalized", () => {
  const [limits, failure] = makeSanitizer({ roots: [] }).records([
    {
      direction: "in",
      frame: {
        method: "account/rateLimits/updated",
        params: {
          rateLimits: {
            primary: {
              usedPercent: 43,
              windowDurationMins: 300,
              resetsAt: 1795869952,
            },
            secondary: {
              usedPercent: 9,
              windowDurationMins: 10080,
              resetsAt: 1796244039,
            },
            credits: { hasCredits: false, balance: "12.5" },
          },
        },
      },
    },
    {
      direction: "in",
      frame: {
        method: "error",
        params: {
          error: {
            additionalDetails:
              "401 Unauthorized, cf-ray: a618907869f55a00-DEL, request id: req_4bafe0d5b7364c218be5f0e5cd24c99d",
          },
        },
      },
    },
  ]);
  const { primary, secondary, credits } = limits.frame.params.rateLimits;
  assert.deepEqual(primary, {
    usedPercent: 10,
    windowDurationMins: 300,
    resetsAt: 1800018000,
  });
  assert.deepEqual(secondary, {
    usedPercent: 10,
    windowDurationMins: 10080,
    resetsAt: 1800604800,
  });
  assert.equal(credits.balance, "0");
  const [claude] = makeSanitizer({ roots: [] }).records([
    {
      direction: "in",
      frame: {
        type: "rate_limit_event",
        rate_limit_info: {
          status: "allowed_warning",
          utilization: 0.9,
          resetsAt: 1795849200,
        },
      },
    },
  ]);
  assert.deepEqual(claude.frame.rate_limit_info, {
    status: "allowed_warning",
    utilization: 0.1,
    resetsAt: 1800000000,
  });
  assert.equal(
    failure.frame.params.error.additionalDetails,
    "401 Unauthorized, cf-ray: 0000000000000000-XXX, request id: req_00000000000000000000000000000000",
  );
});
