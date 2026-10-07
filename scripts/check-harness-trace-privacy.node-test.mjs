import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

import { privacyFailuresForRecords } from "./check-harness-trace-privacy.mjs";

test("fixture privacy guard rejects a foreign absolute path", () => {
  const failures = privacyFailuresForRecords(
    [{ frame: { params: { cwd: String.raw`D:\private-build\workspace` } } }],
    true,
  );

  assert.deepEqual(failures, ["/0/frame/params/cwd"]);
});

test("fixture privacy guard permits public HTTPS URLs", () => {
  const failures = privacyFailuresForRecords(
    [{ frame: { params: { url: "https://example.test/acp/trace" } } }],
    true,
  );

  assert.deepEqual(failures, []);
});

test("fixture privacy guard rejects profile paths beneath the synthetic root", () => {
  const failures = privacyFailuresForRecords(
    [
      {
        frame: {
          params: { cwd: String.raw`C:\fixture-workspace\Users\alice\project` },
        },
      },
    ],
    true,
  );

  assert.deepEqual(failures, ["/0/frame/params/cwd"]);
});

test("fixture privacy guard rejects traversal beneath the synthetic root", () => {
  const failures = privacyFailuresForRecords(
    [
      {
        frame: {
          params: { cwd: String.raw`C:\fixture-workspace\project\..\private` },
        },
      },
    ],
    true,
  );

  assert.deepEqual(failures, ["/0/frame/params/cwd"]);
});

test("Gemini trace metadata keeps OAuth query values private", () => {
  const metadata = JSON.parse(
    readFileSync(
      join(
        process.cwd(),
        "src-tauri",
        "tests",
        "fixtures",
        "harness-traces",
        "gemini-cli",
        "smoke",
        "frames.jsonl.meta.json",
      ),
      "utf8",
    ),
  );

  assert.match(metadata.stderr, /https:\/\/accounts\.google\.com\//u);
  assert.doesNotMatch(metadata.stderr, /(?:state|client_id|redirect_uri)=/u);
});
