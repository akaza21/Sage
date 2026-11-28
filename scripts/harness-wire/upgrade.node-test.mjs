import assert from "node:assert/strict";
import test from "node:test";

import { report } from "./upgrade.mjs";

const contract = (version, init) => ({
  harness: "codex",
  version,
  in: { "notify:turn/started": ["Started"] },
  out: {},
  types: { Started: init },
});

test("a newer release that changed nothing is not a protocol change", () => {
  assert.deepEqual(
    report(
      contract("0.167.1", { turn: [] }),
      contract("0.172.0", { turn: [] }),
    ),
    {
      changed: false,
      lines: [
        "protocol changes in codex 0.167.1 -> 0.172.0: no protocol changes",
      ],
    },
  );
});

test("a newer release's added field is a protocol change", () => {
  assert.deepEqual(
    report(
      contract("0.167.1", { turn: [] }),
      contract("0.172.0", { turn: [], threadId: [] }),
    ),
    {
      changed: true,
      lines: [
        "protocol changes in codex 0.167.1 -> 0.172.0",
        "in notify:turn/started: + threadId",
      ],
    },
  );
});

test("the same release regenerating differently is a change, not up to date", () => {
  const checked = contract("0.167.1", { turn: [] });
  assert.deepEqual(report(checked, contract("0.167.1", { turn: [] })), {
    changed: false,
    lines: ["codex: no newer release"],
  });
  assert.deepEqual(report(checked, contract("0.167.1", {})), {
    changed: true,
    lines: [
      "codex 0.167.1: the regenerated contract differs from the checked one",
      "in notify:turn/started: - turn",
    ],
  });
});

test("a field that turns open is a protocol change", () => {
  assert.deepEqual(
    report(
      contract("0.167.1", { payload: [] }),
      contract("0.172.0", { payload: ["*"] }),
    ),
    {
      changed: true,
      lines: [
        "protocol changes in codex 0.167.1 -> 0.172.0",
        "in notify:turn/started: + payload.*",
      ],
    },
  );
});
