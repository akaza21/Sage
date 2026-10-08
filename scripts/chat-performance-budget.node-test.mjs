import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

test("CP-24 retained Chat envelope stays within browser performance budgets", () => {
  const cli = path.join(root, "node_modules", "@playwright", "test", "cli.js");
  const result = spawnSync(
    process.execPath,
    [
      cli,
      "test",
      "tests/ui/chat-performance.visual.ts",
      "--config",
      "playwright.config.ts",
      "--grep",
      "cap chat performance envelope",
    ],
    {
      cwd: root,
      encoding: "utf8",
      timeout: 120_000,
      maxBuffer: 4 * 1024 * 1024,
    },
  );
  assert.equal(result.error, undefined, result.error?.message);
  assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
});
