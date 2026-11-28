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

test("a comparison past a hard cap stops loudly instead of enumerating", () => {
  const contract = (version, kinds, types) => ({
    harness: "pi",
    version,
    in: kinds,
    out: {},
    types,
  });
  const compare = (kinds, types) =>
    diffContracts(
      contract("1.5.0", kinds, types),
      contract("1.7.0", kinds, types),
    );
  // 100 kinds sharing one 14-level graph with two fields per level.
  const fan = { T14: { leaf: [] } };
  for (let level = 0; level < 14; level += 1)
    fan[`T${level}`] = { a: [`T${level + 1}`], b: [`T${level + 1}`] };
  const kinds = Object.fromEntries(
    Array.from({ length: 100 }, (_, index) => [`kind${index}`, ["T0"]]),
  );
  assert.throws(() => compare(kinds, fan), /field paths walked exceed 2000000/);
  // A chain nested deeper than any harness nests its fields.
  const chain = { C20: { leaf: [] } };
  for (let level = 0; level < 20; level += 1)
    chain[`C${level}`] = { next: [`C${level + 1}`] };
  assert.throws(
    () => compare({ start: ["C0"] }, chain),
    /nested fields exceed 16/,
  );
  // More changed kinds than a report holds.
  const added = Object.fromEntries(
    Array.from({ length: 5_000 }, (_, index) => [`new${index}`, ["*"]]),
  );
  assert.throws(
    () =>
      diffContracts(contract("1.5.0", {}, {}), contract("1.7.0", added, {})),
    /report lines exceed 5000/,
  );
});
