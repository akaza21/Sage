import { readFileSync, readdirSync } from "node:fs";
import { join, relative } from "node:path";
import process from "node:process";
import { pathToFileURL } from "node:url";

const root = process.cwd();
const tracesRoot = join(
  root,
  "src-tauri",
  "tests",
  "fixtures",
  "harness-traces",
);
const permittedWindowsRoot = /^C:\\fixture-workspace(?:\\|$)/iu;
const windowsAbsolutePath =
  /(?:[A-Za-z]:\\|[A-Za-z]:\/(?!\/)|\\\\[^\\/]+[\\/][^\\/]+[\\/])/gu;
const posixAbsolutePath =
  /(?:file:\/\/)?\/(?:Users|home|private|tmp|var|opt|workspace|workspaces|build|builds)(?:\/|$)/u;

function fixtureFiles(directory) {
  const files = [];
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const file = join(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...fixtureFiles(file));
    } else if (entry.name === "frames.jsonl" || entry.name.endsWith(".json")) {
      files.push(file);
    }
  }
  return files;
}

function pointerPart(key) {
  return String(key).replaceAll("~", "~0").replaceAll("/", "~1");
}

function isPermittedWindowsPath(value) {
  if (!permittedWindowsRoot.test(value)) {
    return false;
  }
  const remainder = value.slice("C:\\fixture-workspace".length);
  return !/(?:^|\\)(?:Users(?:\\|$)|\.\.(?:\\|$))/iu.test(remainder);
}

function hasDisallowedPath(value) {
  for (const match of value.matchAll(windowsAbsolutePath)) {
    if (!isPermittedWindowsPath(value.slice(match.index))) {
      return true;
    }
  }
  return posixAbsolutePath.test(value);
}

function inspect(value, pointer, failures) {
  if (typeof value === "string") {
    if (hasDisallowedPath(value)) {
      failures.push(pointer || "/");
    }
    return;
  }
  if (Array.isArray(value)) {
    value.forEach((entry, index) =>
      inspect(entry, `${pointer}/${index}`, failures),
    );
    return;
  }
  if (value && typeof value === "object") {
    Object.entries(value).forEach(([key, entry]) =>
      inspect(entry, `${pointer}/${pointerPart(key)}`, failures),
    );
  }
}

export function privacyFailuresForRecords(records, jsonLines) {
  const failures = [];
  records.forEach((record, index) => {
    const pointers = [];
    inspect(record, jsonLines ? `/${index}` : "", pointers);
    failures.push(...pointers);
  });
  return failures;
}

export function privacyFailuresForFixtures(directory, workspaceRoot = root) {
  const failures = [];
  for (const file of fixtureFiles(directory).sort()) {
    const relativePath = relative(workspaceRoot, file).replaceAll("\\", "/");
    const jsonLines = file.endsWith(".jsonl");
    const records = jsonLines
      ? readFileSync(file, "utf8")
          .split(/\r?\n/u)
          .filter((line) => line.trim().length > 0)
          .map((line) => JSON.parse(line))
      : [JSON.parse(readFileSync(file, "utf8"))];
    privacyFailuresForRecords(records, jsonLines).forEach((pointer) =>
      failures.push(`${relativePath}${pointer}`),
    );
  }
  return failures;
}

function main() {
  const failures = privacyFailuresForFixtures(tracesRoot);
  if (failures.length > 0) {
    console.error(
      `Harness trace privacy check found ${failures.length} disallowed path value(s):`,
    );
    failures.forEach((failure) => console.error(`- ${failure}`));
    process.exitCode = 1;
  }
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(process.argv[1]).href
) {
  main();
}
