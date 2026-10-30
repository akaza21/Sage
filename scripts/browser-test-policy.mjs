import { readFile, readdir } from "node:fs/promises";
import path from "node:path";

export const BROWSER_POLICY_LIMITS = Object.freeze({
  files: 20_000,
  depth: 16,
  fileBytes: 512 * 1024,
});
const SCAN_ROOTS = ["src", "src-tauri/src", "src-tauri/tests", "tests/ui"];
const TEST_FILE =
  /(\.test\.tsx?|\.visual\.ts|\.spec\.ts|(^|\/)tests?\.rs|_tests?\.rs)$|^src-tauri\/tests\/|^tests\/ui\/shared\//u;
const BROWSER_PATH = /browser/iu;
const TS_BYPASS = [
  [
    /\b(?:test|it|describe)(?:\.describe)?\.(?:skip|only|fixme|todo|fails?|skipIf|runIf)\b/u,
    "skips, narrows or inverts a test",
  ],
  [/\b(?:context|testInfo)\.(?:skip|fixme|fail)\s*\(/u, "skips a running test"],
  [/\bx(?:it|describe|test)\s*\(/u, "disables a test"],
  [/\bretries\s*[:,}=]|--retries\b/u, "retries a browser test"],
];
const RUST_BYPASS = [
  [/#\[(?:cfg_attr\([^\]]*)?\bignore\b/u, "ignores a Rust test"],
];
export const RETRY_GUARD = "forbidBrowserRetries(";
export const UI_RULE_DIRECTORY = "docs/work-items/BROWSER-FIRST-CLASS/ui-rules";
export const UI_RULE_MAX_TOLERANCE_PX = 4;
const UI_RULE_MEASURES = {
  height: "number",
  width: "number",
  "background-color": "string",
  "border-radius": "string",
  "border-top-style": "string",
  "border-top-width": "string",
  "text-align": "string",
  placeholder: "string",
  "left-of": "other",
  below: "other",
  "right-inset": "number",
  "left-inset": "number",
  "gap-below": "number",
};
const REFERENCE_DIRECTORY = "docs/work-items/CODEX-PARITY/reference";

async function filesBelow(root, relativeDirectory, budget, depth = 0) {
  if (depth > BROWSER_POLICY_LIMITS.depth)
    throw new Error(`${relativeDirectory} exceeds the policy walk depth.`);
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
    if (entry.name === "node_modules" || entry.name === "target") continue;
    const relativePath = `${relativeDirectory}/${entry.name}`;
    if (entry.isDirectory())
      files.push(...(await filesBelow(root, relativePath, budget, depth + 1)));
    else if (entry.isFile()) {
      budget.files += 1;
      if (budget.files > BROWSER_POLICY_LIMITS.files)
        throw new Error("browser policy walk exceeds its file cap.");
      files.push(relativePath);
    }
  }
  return files;
}

async function readBounded(root, relativePath) {
  const source = await readFile(path.join(root, relativePath), "utf8");
  if (Buffer.byteLength(source) > BROWSER_POLICY_LIMITS.fileBytes)
    throw new Error(`${relativePath} exceeds the policy file byte cap.`);
  return source;
}

function isCommentLine(line) {
  return /^\s*(\/\/|\/\*|\*)/u.test(line);
}

export function bypassErrors(relativePath, source) {
  const patterns = relativePath.endsWith(".rs") ? RUST_BYPASS : TS_BYPASS;
  const errors = [];
  source.split("\n").forEach((line, index) => {
    if (isCommentLine(line)) return;
    for (const [pattern, reason] of patterns) {
      if (pattern.test(line))
        errors.push(
          `${relativePath}:${index + 1} ${reason}; browser checks may not be bypassed.`,
        );
    }
  });
  return errors;
}

export function retryGuardErrors(relativePath, source) {
  return /^tests\/ui\/browser-[^/]+\.visual\.ts$/u.test(relativePath) &&
    !source.includes(RETRY_GUARD)
    ? [
        `${relativePath} must call ${RETRY_GUARD}) so CI retries cannot hide a failure.`,
      ]
    : [];
}

export function uiRuleErrors(relativePath, ruleSet, referenceNames) {
  const errors = [];
  const ids = new Set();
  const fail = (rule, message) =>
    errors.push(`${relativePath} rule ${rule.id ?? "?"} ${message}`);
  for (const rule of ruleSet.rules ?? []) {
    if (ids.has(rule.id)) fail(rule, "is repeated.");
    ids.add(rule.id);
    const reference = /^reference\/(\d{2}[a-z]?)$/u.exec(rule.ref ?? "")?.[1];
    if (
      !reference ||
      !referenceNames.some((name) => name.startsWith(`${reference}-`))
    )
      fail(rule, "cites no Codex reference image on disk.");
    if (typeof rule.source !== "string" || rule.source.trim() === "")
      fail(rule, "does not say where its value was measured.");
    if (typeof rule.selector !== "string" || rule.selector.trim() === "")
      fail(rule, "has no selector.");
    const kind = UI_RULE_MEASURES[rule.measure];
    if (!kind) fail(rule, `uses unknown measure ${rule.measure}.`);
    else if (
      kind === "other"
        ? typeof rule.other !== "string"
        : typeof rule.expected !== kind
    )
      fail(
        rule,
        `needs a ${kind === "other" ? "comparison selector" : `${kind} expectation`}.`,
      );
    if (
      (rule.measure === "right-inset" || rule.measure === "left-inset") &&
      typeof rule.within !== "string"
    )
      fail(rule, "needs a containing selector.");
    if (rule.measure === "gap-below" && typeof rule.other !== "string")
      fail(rule, "needs a comparison selector.");
    const tolerance = rule.tolerancePx ?? 0;
    if (
      !Number.isFinite(tolerance) ||
      tolerance < 0 ||
      tolerance > UI_RULE_MAX_TOLERANCE_PX
    )
      fail(
        rule,
        `allows ${tolerance} px (limit 0 to ${UI_RULE_MAX_TOLERANCE_PX}).`,
      );
  }
  return errors;
}

export async function browserTestPolicyErrors(root) {
  const errors = [];
  const budget = { files: 0 };
  for (const directory of SCAN_ROOTS) {
    for (const file of await filesBelow(root, directory, budget)) {
      if (!BROWSER_PATH.test(file)) continue;
      const rustUnderBrowser = file.endsWith(".rs");
      if (!TEST_FILE.test(file) && !rustUnderBrowser) continue;
      const source = await readBounded(root, file);
      errors.push(
        ...bypassErrors(file, source),
        ...retryGuardErrors(file, source),
      );
    }
  }
  const referenceNames = (
    await filesBelow(root, REFERENCE_DIRECTORY, { files: 0 })
  ).map((file) => path.posix.basename(file));
  for (const file of await filesBelow(root, UI_RULE_DIRECTORY, { files: 0 })) {
    if (!file.endsWith(".json")) continue;
    let ruleSet;
    try {
      ruleSet = JSON.parse(await readBounded(root, file));
    } catch (error) {
      errors.push(`${file} is not valid JSON: ${error.message}`);
      continue;
    }
    errors.push(...uiRuleErrors(file, ruleSet, referenceNames));
  }
  return errors;
}
