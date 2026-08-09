import { lstatSync, readdirSync, realpathSync } from "node:fs";
import path from "node:path";

import {
  exactKeys,
  parseBoundedJson,
  safeInteger,
  SHA256,
} from "./ade0006a-c0b-common.mjs";

function expectedInventory(frozen) {
  return frozen.budgets.measurementContract.observationFiles.inventory;
}

export function loadObservationRecords(rawRoot, descriptors, frozen) {
  const before = lstatSync(rawRoot);
  if (!before.isDirectory() || before.isSymbolicLink()) {
    throw new Error("C0-B observation root must be a regular directory");
  }
  const canonical = realpathSync.native(rawRoot);
  const actual = readdirSync(canonical).sort((left, right) =>
    Buffer.from(left).compare(Buffer.from(right)),
  );
  const expected = [...expectedInventory(frozen)].sort((left, right) =>
    Buffer.from(left).compare(Buffer.from(right)),
  );
  if (actual.join("\0") !== expected.join("\0")) {
    throw new Error("C0-B observation root inventory is not exact");
  }
  if (
    !Array.isArray(descriptors) ||
    descriptors.map(({ relativePath }) => relativePath).join("\0") !==
      expectedInventory(frozen).join("\0")
  ) {
    throw new Error(
      "C0-B observation descriptors are incomplete or out of order",
    );
  }
  const records = new Map();
  for (const descriptor of descriptors) {
    const record = parseBoundedJson(
      path.join(canonical, descriptor.relativePath),
      frozen.evidence.campaignBounds.curatedBytes,
      frozen.limits.wire,
      `C0-B observation ${descriptor.relativePath}`,
    );
    if (
      record.bytes !== descriptor.bytes ||
      record.sha256 !== descriptor.sha256
    ) {
      throw new Error(`C0-B observation ${descriptor.relativePath} changed`);
    }
    records.set(descriptor.relativePath, record);
  }
  return records;
}

export function validateObservationRecords(errors, results, frozen, records) {
  const inventory = expectedInventory(frozen);
  const descriptors = results?.observationFiles;
  if (
    !Array.isArray(descriptors) ||
    descriptors.length !== inventory.length ||
    descriptors.map(({ relativePath }) => relativePath).join("\0") !==
      inventory.join("\0")
  ) {
    errors.push("results.observationFiles is incomplete or out of order");
    return new Map();
  }
  const seen = new Set();
  let bytes = 0;
  descriptors.forEach((descriptor, index) => {
    const label = `results.observationFiles[${index}]`;
    exactKeys(errors, descriptor, ["relativePath", "bytes", "sha256"], label);
    safeInteger(
      errors,
      descriptor?.bytes,
      `${label}.bytes`,
      1,
      frozen.evidence.campaignBounds.curatedBytes,
    );
    if (!SHA256.test(descriptor?.sha256 ?? "")) {
      errors.push(`${label}.sha256 is invalid`);
    }
    const record = records?.get(descriptor?.relativePath);
    if (
      !record ||
      record.bytes !== descriptor?.bytes ||
      record.sha256 !== descriptor?.sha256
    ) {
      errors.push(`${label} does not authenticate its observation file`);
    }
    if (seen.has(descriptor?.relativePath)) {
      errors.push(`${label}.relativePath is duplicated`);
    }
    seen.add(descriptor?.relativePath);
    bytes += descriptor?.bytes ?? 0;
  });
  if (
    results?.evidence?.rawFiles !== inventory.length ||
    results?.evidence?.rawBytes !== bytes
  ) {
    errors.push("results.evidence raw file accounting is stale");
  }
  return records;
}
