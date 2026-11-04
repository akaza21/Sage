import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";

// Collect actual inputs while running the existing suite. Browser fixture inputs
// are also included directly by the corpus test, without launching Playwright.
const capture = "work/markdown-corpus-inputs.jsonl";
const fixture = "src/features/files/fixtures/markdown-regression-inputs.json";
mkdirSync("work", { recursive: true });
writeFileSync(capture, "");
const result = spawnSync(
  process.execPath,
  [
    "node_modules/vitest/vitest.mjs",
    "run",
    "--exclude",
    "tests/ui/**/*.spec.ts",
    "--maxWorkers=2",
  ],
  {
    env: { ...process.env, SAGE_MARKDOWN_CORPUS_RECORD: "1" },
    stdio: "inherit",
  },
);
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status ?? 1);
const inputs = JSON.parse(readFileSync(fixture, "utf8"));
const collected = new Map();
for (const line of readFileSync(capture, "utf8").trim().split("\n")) {
  const input = JSON.parse(line);
  if (!collected.has(input.markdown)) collected.set(input.markdown, input);
}
// Keep established names/order stable when the test runner changes file order.
inputs.testInputs = [
  ...inputs.testInputs,
  ...[...collected.values()]
    .filter(
      ({ markdown }) =>
        !inputs.testInputs.some((input) => input.markdown === markdown),
    )
    .sort((a, b) => a.markdown.localeCompare(b.markdown, "en")),
];
writeFileSync(fixture, `${JSON.stringify(inputs, null, 2)}\n`);
console.log(`Captured ${collected.size} unique markdown test inputs.`);
