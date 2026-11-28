import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
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
      // The account's usage, its sign-in's expiry, and the provider's
      // request, edge, message and tool-use ids.
      for (const pattern of [
        /cf-ray: (?!0{8,}\d+-XXX)/,
        /req_(?!0{20,}\d+\b)[0-9A-Za-z]{16}/,
        /\b(?:msg|toolu|srvtoolu|call|resp)_(?!0{12})[0-9A-Za-z]{12}/,
        /"credential_expires_at":"(?!2027-01-01T00:00:00\.000Z)/,
        // Normalized, or neutral (0) where the field is undeclared.
        /"usedPercent":(?!(?:10|0)[,}])\d/,
        /"utilization":(?!(?:0\.1|0)[,}])\d/,
        /"resetsAt":(?!(?:180\d{7}|0)[,}])\d/,
        /"(?:hasCredits|unlimited|isUsingOverage|overageInUse|overageEnabled)":true/,
        /"overageDisabledReason":"(?!redacted")/,
        /"overageStatus":"(?!redacted")/,
        /"surpassedThreshold":(?!0[,}])\d/,
        /"(?:email|subscriptionType|apiProvider|organization[A-Za-z]*)":"(?!redacted")/,
      ])
        assert.doesNotMatch(body, pattern, name);
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
            credits: { hasCredits: true, unlimited: true, balance: "12.5" },
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
  assert.deepEqual(credits, {
    hasCredits: false,
    unlimited: false,
    balance: "redacted",
  });
  const [claude] = makeSanitizer({ roots: [] }).records([
    {
      direction: "in",
      frame: {
        type: "rate_limit_event",
        rate_limit_info: {
          status: "allowed_warning",
          rateLimitType: "seven_day",
          utilization: 0.9,
          resetsAt: 1795849200,
          overageStatus: "rejected",
          overageDisabledReason: "out_of_credits",
          isUsingOverage: true,
          surpassedThreshold: 0.75,
        },
      },
    },
  ]);
  assert.deepEqual(claude.frame.rate_limit_info, {
    status: "allowed",
    rateLimitType: "seven_day",
    utilization: 0.1,
    resetsAt: 1800000000,
    overageStatus: "redacted",
    overageDisabledReason: "redacted",
    isUsingOverage: false,
    surpassedThreshold: 0,
  });
  assert.equal(
    failure.frame.params.error.additionalDetails,
    "401 Unauthorized, cf-ray: 0000000000000001-XXX, request id: req_00000000000000000000000000000001",
  );
});

test("provider ids keep their links, a sign-in's expiry is fixed, other payloads are left alone", () => {
  const [use, result, auth, tool, catalog, subagent, claudeCatalog] =
    makeSanitizer({
      roots: [],
    }).records([
      {
        direction: "in",
        frame: {
          type: "assistant",
          message: {
            id: "msg_01AbCdEfGhIjKlMnOp",
            content: [{ type: "tool_use", id: "toolu_01QrStUvWxYz012345" }],
          },
        },
      },
      {
        direction: "in",
        frame: {
          type: "user",
          message: {
            content: [
              { type: "tool_result", tool_use_id: "toolu_01QrStUvWxYz012345" },
            ],
          },
        },
      },
      {
        direction: "in",
        frame: {
          type: "system",
          subtype: "init",
          credential_expires_at: "2026-12-25T11:47:22.873Z",
        },
      },
      // A tool's own input that happens to use usage-like keys.
      {
        direction: "in",
        frame: {
          type: "assistant",
          message: {
            content: [
              {
                type: "tool_use",
                input: { utilization: 0.9, resetsAt: 123 },
                extra: { balance: "125", resetsAt: 123 },
              },
            ],
          },
        },
      },
      {
        direction: "in",
        frame: {
          id: 3,
          result: {
            data: [
              {
                id: "private-a",
                model: "private-a",
                displayName: "A",
                upgrade: "private-b",
              },
            ],
          },
        },
      },
      // A subagent's result names the model it resolved to, outside any catalog.
      {
        direction: "in",
        frame: {
          type: "user",
          tool_use_result: {
            status: "async_launched",
            resolvedModel: "private-d",
          },
        },
      },
      // Claude Code's catalog, in its initialize reply.
      {
        direction: "in",
        frame: {
          type: "control_response",
          response: {
            response: {
              models: [
                {
                  value: "default",
                  resolvedModel: "private-c",
                  description: "Private C · best",
                },
                {
                  value: "private-alias",
                  resolvedModel: "private-c",
                  description: "Private C · best",
                },
              ],
            },
          },
        },
      },
    ]);
  const toolUse = use.frame.message.content[0].id;
  assert.match(use.frame.message.id, /^msg_0+1$/);
  assert.match(toolUse, /^toolu_0+\d$/);
  assert.equal(result.frame.message.content[0].tool_use_id, toolUse);
  assert.equal(auth.frame.credential_expires_at, "2027-01-01T00:00:00.000Z");
  assert.deepEqual(tool.frame.message.content[0].extra, {
    balance: "125",
    resetsAt: 123,
  });
  assert.deepEqual(tool.frame.message.content[0].input, {
    utilization: 0.9,
    resetsAt: 123,
  });
  assert.doesNotMatch(JSON.stringify(catalog), /private/);
  const [, alias] = claudeCatalog.frame.response.response.models;
  assert.equal(
    claudeCatalog.frame.response.response.models[0].value,
    "default",
  );
  assert.match(alias.value, /^model-\d+$/);
  assert.equal(
    alias.resolvedModel,
    claudeCatalog.frame.response.response.models[0].resolvedModel,
  );
  assert.doesNotMatch(JSON.stringify(claudeCatalog), /private|Private/);
  assert.match(subagent.frame.tool_use_result.resolvedModel, /^model-\d+$/);
});

test("a re-scrub that cannot keep every reference leaves the corpus as it was", () => {
  const out = fs.mkdtempSync(path.join(os.tmpdir(), "sage-resanitize-"));
  try {
    // An earlier harness the scrub would change, and a later one with a
    // broken reference.
    const codex = path.join(out, "codex");
    fs.mkdirSync(codex);
    const codexCommands = `${JSON.stringify({ direction: "in", frame: { error: { message: "cf-ray: a618907869f55a00-DEL" } } })}\n`;
    fs.writeFileSync(path.join(codex, "commands.jsonl"), codexCommands);
    const dir = path.join(out, "pi");
    fs.mkdirSync(dir);
    const commands = `${JSON.stringify({ direction: "in", frame: { type: "response", id: "req_0123456789abcdefABCD" } })}\n`;
    const broken = `${JSON.stringify({ direction: "in", ref: "commands.jsonl", sha256: "ffffffffffff" })}\n`;
    fs.writeFileSync(path.join(dir, "commands.jsonl"), commands);
    fs.writeFileSync(path.join(dir, "shell.jsonl"), broken);
    const run = spawnSync(
      process.execPath,
      [
        path.join("scripts", "harness-wire", "capture.mjs"),
        "--out",
        out,
        "--harness",
        "codex,pi",
        "--resanitize",
        "--work",
        path.join(out, "work"),
      ],
      { encoding: "utf8" },
    );
    assert.notEqual(run.status, 0);
    assert.equal(
      fs.readFileSync(path.join(dir, "commands.jsonl"), "utf8"),
      commands,
    );
    assert.equal(
      fs.readFileSync(path.join(dir, "shell.jsonl"), "utf8"),
      broken,
    );
    assert.deepEqual(fs.readdirSync(dir).sort(), [
      "commands.jsonl",
      "shell.jsonl",
    ]);
    assert.equal(
      fs.readFileSync(path.join(codex, "commands.jsonl"), "utf8"),
      codexCommands,
    );
    assert.deepEqual(fs.readdirSync(codex), ["commands.jsonl"]);
  } finally {
    fs.rmSync(out, { recursive: true, force: true });
  }
});

test("a model's id or name is redacted in keys and free text, a command named after the product is not", () => {
  const [usage, offer] = makeSanitizer({ roots: [] }).records([
    {
      direction: "in",
      frame: {
        type: "result",
        modelUsage: {
          "claude-haiku-5-5": { canonicalModel: "claude-haiku-5-5" },
        },
        commands: ["claude-api", "claude-approvals"],
      },
    },
    {
      direction: "in",
      frame: {
        result: { message: "Switch to GPT-6.2 Sol. Fast mode uses Opus 5.5." },
      },
    },
  ]);
  const [key] = Object.keys(usage.frame.modelUsage);
  assert.match(key, /^model-\d+$/);
  assert.equal(usage.frame.modelUsage[key].canonicalModel, key);
  // Product-named commands are not model ids; as catalog entries they get
  // catalog pseudonyms.
  assert.deepEqual(usage.frame.commands, ["command-1", "command-2"]);
  assert.match(
    offer.frame.result.message,
    /^Switch to Model \d+\. Fast mode uses Model \d+\.$/,
  );
});

test("inside an account, rate-limit or catalog object only the keys a binding reads keep their values", () => {
  const [init] = makeSanitizer({ roots: [] }).records([
    {
      direction: "in",
      frame: {
        type: "control_response",
        response: {
          response: {
            account: {
              email: "x",
              subscriptionType: "max",
              apiProvider: "firstParty",
            },
            models: [
              {
                value: "default",
                displayName: "D",
                supportsEffort: true,
                supportsFastMode: true,
                price: 4,
              },
            ],
            commands: [{ name: "review", description: "Review the branch" }],
          },
        },
      },
    },
  ]);
  const reply = init.frame.response.response;
  assert.deepEqual(reply.account, {
    email: "redacted",
    subscriptionType: "redacted",
    apiProvider: "redacted",
  });
  assert.deepEqual(reply.models[0], {
    value: "default",
    displayName: "redacted",
    supportsEffort: true,
    supportsFastMode: false,
    price: 0,
  });
  // A command an account can configure: a pseudonym and no description.
  assert.deepEqual(reply.commands, [
    { name: "command-1", description: "redacted" },
  ]);
});

test("an account object keeps no id, name or date of its own, distinct request ids stay distinct, and turns cost a fixed amount", () => {
  const [account, catalog, turn] = makeSanitizer({ roots: [] }).records([
    {
      direction: "in",
      frame: {
        account: {
          id: "acct_private_918",
          name: "Private Company",
          resets_at: "2026-12-01T00:00:00Z",
          type: "chatgpt",
        },
      },
    },
    {
      direction: "in",
      frame: {
        result: {
          data: [
            {
              id: "private-codex",
              model: "private-codex",
              displayName: "P",
              serviceTiers: [
                {
                  id: "tier-private",
                  name: "Private tier",
                  description: "Private",
                },
              ],
            },
          ],
        },
      },
    },
    {
      direction: "in",
      frame: {
        type: "result",
        result:
          "Ran private-codex twice: req_aaaaaaaaaaaaaaaaaaaaaaaa, req_bbbbbbbbbbbbbbbbbbbbbbbb, req_aaaaaaaaaaaaaaaaaaaaaaaa",
        usage: { input_tokens: 5123, output_tokens: 77 },
        total_cost_usd: 0.37,
      },
    },
  ]);
  assert.deepEqual(account.frame.account, {
    id: "redacted",
    name: "redacted",
    resets_at: "redacted",
    type: "chatgpt",
  });
  assert.deepEqual(catalog.frame.result.data[0].serviceTiers, [
    { id: "redacted", name: "redacted", description: "redacted" },
  ]);
  const [first, second, again] = turn.frame.result.match(/req_\w+/g);
  assert.notEqual(first, second);
  assert.equal(first, again);
  assert.doesNotMatch(turn.frame.result, /private|aaaa|bbbb/);
  assert.deepEqual(turn.frame.usage, { input_tokens: 100, output_tokens: 100 });
  assert.equal(turn.frame.total_cost_usd, 0.01);
});

test("token estimates, agent descriptions, unlisted account paths, display names and absolute times leave no trace", () => {
  const [init, list, turn] = makeSanitizer({ roots: [] }).records([
    {
      direction: "in",
      frame: {
        timestamp: "2026-11-28T09:00:00.000Z",
        agents: [
          { name: "claude", description: "Private Fleet's default agent" },
        ],
        account: { value: "customer-secret", type: "chatgpt" },
      },
    },
    {
      direction: "in",
      frame: {
        emittedAtMs: 1795860000000,
        result: {
          data: [
            {
              id: "p-y",
              model: "p-y",
              displayName: "Private Y",
              isDefault: true,
            },
          ],
        },
      },
    },
    {
      direction: "in",
      frame: {
        emittedAtMs: 1795860002500,
        message: "Selected Private Y",
        thinking_tokens: 186,
        estimated_tokens_delta: 12,
        durationMs: 400,
      },
    },
  ]);
  assert.deepEqual(init.frame.agents, [
    { name: "agent-1", description: "redacted" },
  ]);
  assert.deepEqual(init.frame.account, { value: "redacted", type: "chatgpt" });
  assert.equal(init.frame.timestamp, "2027-01-15T08:00:00.000Z");
  assert.equal(list.frame.result.data[0].isDefault, true);
  assert.equal(turn.frame.emittedAtMs - list.frame.emittedAtMs, 2500);
  assert.equal(turn.frame.durationMs, 400);
  assert.equal(turn.frame.thinking_tokens, 100);
  assert.equal(turn.frame.estimated_tokens_delta, 100);
  assert.doesNotMatch(
    JSON.stringify([init, list, turn]),
    /Private|private|customer|2026-11/,
  );
});

test("configured catalogs, unapproved account keys, run ids and a usage status carry nothing of the account", () => {
  const [init, turn] = makeSanitizer({ roots: [] }).records([
    {
      direction: "in",
      frame: {
        commands: [
          {
            name: "acme-review",
            description: "Review for Acme",
            argumentHint: "[pr]",
          },
        ],
        slash_commands: ["acme-review"],
        agents: [{ name: "acme-expert", description: "Acme" }],
        account: { "acme-client-id": true, type: "chatgpt" },
        rateLimitsByLimitId: {
          "acme-limit": { limitId: "acme-limit", primary: { usedPercent: 5 } },
        },
      },
    },
    {
      direction: "in",
      frame: {
        task_id: "b8nnzn53z",
        description: "Output at /tmp/b8nnzn53z.output",
        subagent_type: "acme-expert",
        rate_limit_info: { status: "allowed_warning", utilization: 0.9 },
      },
    },
  ]);
  assert.deepEqual(init.frame.commands, [
    { name: "command-1", description: "redacted", argumentHint: "redacted" },
  ]);
  assert.deepEqual(init.frame.slash_commands, ["command-1"]);
  assert.equal(turn.frame.subagent_type, init.frame.agents[0].name);
  assert.equal(turn.frame.task_id, "task-1");
  assert.equal(turn.frame.description, "Output at /tmp/task-1.output");
  assert.deepEqual(turn.frame.rate_limit_info, {
    status: "allowed",
    utilization: 0.1,
  });
  assert.doesNotMatch(
    JSON.stringify([init, turn]),
    /acme|Acme|b8nnzn53z|allowed_warning/,
  );
});
