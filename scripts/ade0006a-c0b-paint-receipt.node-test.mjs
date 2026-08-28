import assert from "node:assert/strict";
import test from "node:test";

import { authenticatePaintReceipt } from "./ade0006a-c0b-paint-receipt.mjs";

const WORKED = Object.freeze({
  operation: "invoke",
  startedTick: 0,
  completedTick: 20,
  durationUs: 20_000,
  frequencyHz: 1000,
});

test("authenticatePaintReceipt hashes the canonical paint span from ticks", () => {
  const receipt = authenticatePaintReceipt(WORKED);
  assert.equal(receipt.operation, "invoke");
  assert.equal(receipt.startedTick, 0);
  assert.equal(receipt.completedTick, 20);
  assert.equal(receipt.durationUs, 20_000);
  assert.equal(
    receipt.sha256,
    "323b0f29657d66e23bd429594467c36dbdb3d6436a8e796a9e1f0adce621df14",
  );
});

test("authenticatePaintReceipt rejects a duration that does not match the ticks", () => {
  assert.throws(
    () =>
      authenticatePaintReceipt({
        ...WORKED,
        durationUs: 20_001,
      }),
    /paint-receipt-invalid/,
  );
});

test("authenticatePaintReceipt rejects an operation outside the semantic set", () => {
  assert.throws(
    () =>
      authenticatePaintReceipt({
        ...WORKED,
        operation: "click",
      }),
    /paint-receipt-invalid/,
  );
});
