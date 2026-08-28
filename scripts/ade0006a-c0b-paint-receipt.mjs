import { exactKeys, recordDigest, SHA256 } from "./ade0006a-c0b-common.mjs";
import { monotonicDurationUs } from "./ade0006a-c0b-observation.mjs";

export const PAINT_OPERATIONS = Object.freeze([
  "focus",
  "invoke",
  "select",
  "set-value",
  "toggle",
]);

export const PAINT_RECEIPT_KEYS = Object.freeze([
  "completedTick",
  "durationUs",
  "operation",
  "sha256",
  "startedTick",
]);

function invalid() {
  throw new Error("paint-receipt-invalid");
}

export function authenticatePaintReceipt({
  operation,
  startedTick,
  completedTick,
  durationUs,
  frequencyHz,
}) {
  if (!PAINT_OPERATIONS.includes(operation)) invalid();
  if (
    durationUs !== monotonicDurationUs(startedTick, completedTick, frequencyHz)
  ) {
    invalid();
  }
  const body = {
    completedTick,
    durationUs,
    operation,
    startedTick,
  };
  return Object.freeze({
    ...body,
    sha256: recordDigest(body),
  });
}

function receiptDigest(value) {
  return `${value?.operation ?? ""}\0${value?.startedTick ?? ""}\0${value?.completedTick ?? ""}\0${value?.durationUs ?? ""}\0${value?.sha256 ?? ""}`;
}

export function validatePaintReceipt(
  errors,
  receipt,
  expected,
  frequencyHz,
  label,
) {
  exactKeys(errors, receipt, PAINT_RECEIPT_KEYS, label);
  if (!SHA256.test(receipt?.sha256 ?? "")) {
    errors.push(`${label}.sha256 is invalid`);
    return;
  }
  let authenticated;
  try {
    authenticated = authenticatePaintReceipt({
      operation: receipt?.operation,
      startedTick: expected.startedTick,
      completedTick: expected.completedTick,
      durationUs: expected.durationUs,
      frequencyHz,
    });
  } catch {
    errors.push(`${label} is invalid`);
    return;
  }
  if (receiptDigest(receipt) !== receiptDigest(authenticated)) {
    errors.push(`${label} is invalid`);
  }
}
