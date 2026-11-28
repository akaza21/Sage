import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";

import {
  CONTRACTS,
  checkCorpus,
  contracts,
  diffContracts,
} from "./contract.mjs";

test("every recorded frame matches its harness version's contract and reviewed list", () => {
  const checked = fs
    .readdirSync(CONTRACTS)
    .filter((harness) => contracts(harness).length);
  assert.ok(checked.length >= 2, `contracts found for ${checked}`);
  assert.deepEqual(checkCorpus(), []);
});

test("an upgrade reports added and removed kinds and fields", () => {
  const contract = (version, init, types) => ({
    harness: "claude",
    version,
    in: { "system/init": ["Init"], ...init },
    out: {},
    types: { Init: types, Plugin: { name: [] } },
  });
  const before = contract(
    "2.1.330",
    { keep_alive: ["Alive"] },
    { model: [], plugins: ["Plugin"] },
  );
  before.types.Alive = { type: [] };
  const after = contract(
    "2.1.332",
    {},
    { model: [], is_backgrounded: [], plugins: ["*"] },
  );
  assert.deepEqual(diffContracts(before, after), [
    "protocol changes in claude 2.1.330 -> 2.1.332",
    "in removed keep_alive",
    "in system/init: - plugins.name",
    "in system/init: + is_backgrounded",
    "in system/init: + plugins.*",
  ]);
});

test("a type graph whose field paths fan out is reported, not enumerated", () => {
  const types = {};
  for (let level = 0; level < 25; level += 1)
    types[`T${level}`] = { a: [`T${level + 1}`], b: [`T${level + 1}`] };
  types.T25 = { leaf: [] };
  const contract = {
    harness: "pi",
    version: "1.5.0",
    in: { start: ["T0"] },
    out: {},
    types,
  };
  assert.throws(
    () => diffContracts(contract, { ...contract, version: "1.7.0" }),
    /field paths below .* exceed 200000/,
  );
});
