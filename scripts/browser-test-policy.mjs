import { readFile, readdir } from "node:fs/promises";
import path from "node:path";

const TEST_ROOTS = [
  "tests/ui",
  "src/features/browser",
  "src/features/preview",
  "src/app",
  "src/features/chat/acp",
  "src-tauri/src/browser",
  "src-tauri/src/acp/browser",
];
const BROWSER_TEST_FILE =
  /(^tests\/ui\/browser-[^/]+\.ts$)|(^src\/features\/browser\/.+\.test\.tsx?$)|(^src\/features\/preview\/HarnessBrowser[^/]*\.test\.tsx?$)|(^src\/app\/browser[^/]*\.test\.ts$)|(^src\/features\/chat\/acp\/Browser[^/]*\.test\.tsx?$)|(^src-tauri\/src\/(acp\/)?browser\/.+\.rs$)/u;
const BYPASS_PATTERNS = [
  [
    /\b(?:test|it|describe)\.(?:skip|only|fixme|todo)\b/u,
    "skips or narrows a test",
  ],
  [
    /\btest\.fail\s*\(|\bcontext\.skip\s*\(/u,
    "marks a test as expected to fail or skipped",
  ],
  [/#\[ignore\b/u, "ignores a Rust test"],
  [/\bretries\s*:/u, "retries a browser test"],
];
export const UI_RULE_DIRECTORY = "docs/work-items/BROWSER-FIRST-CLASS/ui-rules";
export const UI_RULE_MAX_TOLERANCE_PX = 4;
const REFERENCE_DIRECTORY = "docs/work-items/CODEX-PARITY/reference";

async function filesBelow(root, relativeDirectory) {
  let entries;
  try {
    entries = await readdir(path.join(root, relativeDirectory), {
      withFileTypes: true,
    });
  } catch (error) {
    if (error.code === "ENOENT") return [];
    throw error;
  }
  const files = [];
  for (const entry of entries) {
    const relativePath = `${relativeDirectory}/${entry.name}`;
    if (entry.isDirectory())
      files.push(...(await filesBelow(root, relativePath)));
    else files.push(relativePath);
  }
  return files;
}

export function bypassErrors(relativePath, source) {
  const errors = [];
  source.split("\n").forEach((line, index) => {
    const code = line.replace(/\/\/.*$/u, "");
    for (const [pattern, reason] of BYPASS_PATTERNS) {
      if (pattern.test(code))
        errors.push(
          `${relativePath}:${index + 1} ${reason}; browser checks may not be bypassed.`,
        );
    }
  });
  return errors;
}

export function uiRuleErrors(relativePath, ruleSet, referenceNames) {
  const errors = [];
  const ids = new Set();
  for (const rule of ruleSet.rules ?? []) {
    if (ids.has(rule.id))
      errors.push(`${relativePath} repeats rule ${rule.id}.`);
    ids.add(rule.id);
    const reference = /^reference\/(\d{2})$/u.exec(rule.ref ?? "")?.[1];
    if (
      !reference ||
      !referenceNames.some((name) => name.startsWith(`${reference}-`))
    )
      errors.push(
        `${relativePath} rule ${rule.id} cites no Codex reference image on disk.`,
      );
    if (!rule.source)
      errors.push(
        `${relativePath} rule ${rule.id} does not say where its value was measured.`,
      );
    if ((rule.tolerancePx ?? 0) > UI_RULE_MAX_TOLERANCE_PX)
      errors.push(
        `${relativePath} rule ${rule.id} allows ${rule.tolerancePx} px (limit ${UI_RULE_MAX_TOLERANCE_PX}).`,
      );
  }
  return errors;
}

export async function browserTestPolicyErrors(root) {
  const errors = [];
  for (const directory of TEST_ROOTS) {
    for (const file of await filesBelow(root, directory)) {
      if (!BROWSER_TEST_FILE.test(file)) continue;
      const source = await readFile(path.join(root, file), "utf8");
      errors.push(...bypassErrors(file, source));
    }
  }
  const referenceNames = (await filesBelow(root, REFERENCE_DIRECTORY)).map(
    (file) => path.posix.basename(file),
  );
  for (const file of await filesBelow(root, UI_RULE_DIRECTORY)) {
    if (!file.endsWith(".json")) continue;
    const ruleSet = JSON.parse(await readFile(path.join(root, file), "utf8"));
    errors.push(...uiRuleErrors(file, ruleSet, referenceNames));
  }
  return errors;
}
