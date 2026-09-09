import { expect, it } from "vitest";
import { safeFileChange } from "./acpDesktopResponsePayloads";

const base = {
  resource: {
    kind: "resource",
    resource: { rootId: "root", relativePath: "src/a.ts" },
  },
  operation: "updated",
  originalResource: null,
  summary: "Changed file",
  additions: 1,
  deletions: 1,
};
const pair = {
  pairState: "available",
  snapshotRevision: "1",
  oldBytes: 262144,
  newBytes: 262144,
  contentDigest: "a".repeat(64),
};

it("preserves the bounded immutable pair metadata supplied by native", () => {
  expect(safeFileChange({ ...base, ...pair })).toMatchObject(pair);
});

it("rejects partial, malformed and over-cap pair metadata instead of exposing an action", () => {
  for (const change of [
    { oldBytes: 262145 },
    { newBytes: 262145 },
    { oldBytes: -1 },
    { snapshotRevision: "0" },
    { snapshotRevision: 1 },
    { contentDigest: null },
    { pairState: undefined },
  ]) {
    expect(safeFileChange({ ...base, ...pair, ...change })).toBeNull();
  }
  expect(
    safeFileChange({
      ...base,
      ...pair,
      resource: { kind: "unavailable", reason: "missing", label: "missing" },
    }),
  ).toBeNull();
});

it("accepts legacy unavailable summaries and truthful summary-only overflow", () => {
  expect(safeFileChange(base)).toEqual(base);
  for (const pairState of ["unavailable", "summaryTruncated"]) {
    const metadata = {
      pairState,
      snapshotRevision: "0",
      oldBytes: null,
      newBytes: null,
      contentDigest: null,
    };
    expect(safeFileChange({ ...base, ...metadata })).toMatchObject(metadata);
    expect(safeFileChange({ ...base, ...metadata, oldBytes: 1 })).toBeNull();
  }
});
