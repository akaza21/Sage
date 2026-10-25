import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

// Deterministic synthetic workspace for editor search, path-index and idle
// budgets (EDIT-0005 EP-00, reused by EP-06/07/09/22). The same seed and count
// always give the same paths and contents, on every platform, so timings taken
// on different days compare the same tree.
//
// Search oracles: `visibleNeedleFiles` is what a .gitignore-aware search must
// find; `needleFiles` is what a walker that ignores .gitignore finds. The tree
// has no .git folder, so tools that honour .gitignore only inside a Git
// repository (ripgrep, the `ignore` crate by default) need `git init` first.

export const DEFAULT_SYNTHETIC_FILES = 200_000;
export const MAX_SYNTHETIC_FILES = 1_000_000;
/** One file in this many carries the search needle. */
export const NEEDLE_EVERY = 1_000;
export const SYNTHETIC_NEEDLE = "SAGE_SYNTHETIC_NEEDLE";
/** Directories a .gitignore-aware walker must skip. */
export const IGNORED_DIRECTORIES = Object.freeze([
  "node_modules",
  "target",
  "dist",
]);

const MAX_DEPTH = 7;
const NEW_DIRECTORY_CHANCE = 0.08;
/** Share of files placed under ignored folders, like a built workspace. */
const IGNORED_SHARE = 0.1;
const WORDS = Object.freeze([
  "app",
  "core",
  "editor",
  "panel",
  "store",
  "theme",
  "search",
  "index",
  "git",
  "diff",
  "layout",
  "shell",
  "view",
  "model",
  "util",
  "parse",
  "render",
  "worker",
  "cache",
  "event",
]);
const EXTENSIONS = Object.freeze([
  ["ts", 30],
  ["tsx", 14],
  ["js", 10],
  ["json", 8],
  ["md", 8],
  ["rs", 10],
  ["py", 6],
  ["css", 6],
  ["txt", 4],
  ["png", 4],
]);
const EXTENSION_WEIGHT = EXTENSIONS.reduce(
  (sum, [, weight]) => sum + weight,
  0,
);

/** mulberry32: small, fast and identical on every JavaScript engine. */
function random(seed) {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let value = state;
    value = Math.imul(value ^ (value >>> 15), value | 1);
    value ^= value + Math.imul(value ^ (value >>> 7), value | 61);
    return ((value ^ (value >>> 14)) >>> 0) / 4294967296;
  };
}

function validate({ seed, count }) {
  if (!Number.isInteger(seed) || seed < 0 || seed > 0xffffffff)
    throw new Error("synthetic-tree: seed must be an integer from 0 to 2^32-1");
  if (!Number.isInteger(count) || count < 1 || count > MAX_SYNTHETIC_FILES)
    throw new Error(
      `synthetic-tree: count must be an integer from 1 to ${MAX_SYNTHETIC_FILES}`,
    );
}

/**
 * Yields `{ path, content }` for every file, in a stable order. Paths are
 * relative and use "/". Names carry their index, so every path is unique.
 */
export function* syntheticTree({
  seed = 1,
  count = DEFAULT_SYNTHETIC_FILES,
} = {}) {
  validate({ seed, count });
  const next = random(seed);
  const pick = (list) => list[Math.floor(next() * list.length)];
  // Separate pools, so each ignored folder holds a steady share of files
  // instead of compounding: a new folder joins its parent's pool.
  const workspace = [{ path: "", depth: 0 }];
  const ignored = IGNORED_DIRECTORIES.map((name) => [{ path: name, depth: 1 }]);
  let folders = 1 + ignored.length;
  yield {
    path: ".gitignore",
    content: IGNORED_DIRECTORIES.map((name) => `${name}/\n`).join(""),
  };
  for (let index = 1; index < count; index += 1) {
    const directories = next() < IGNORED_SHARE ? pick(ignored) : workspace;
    let parent = pick(directories);
    while (next() < NEW_DIRECTORY_CHANCE && parent.depth < MAX_DEPTH) {
      const name = `${pick(WORDS)}${folders}`;
      folders += 1;
      parent = {
        path: parent.path ? `${parent.path}/${name}` : name,
        depth: parent.depth + 1,
      };
      directories.push(parent);
    }
    let roll = next() * EXTENSION_WEIGHT;
    let extension = EXTENSIONS[0][0];
    for (const [candidate, weight] of EXTENSIONS) {
      roll -= weight;
      if (roll < 0) {
        extension = candidate;
        break;
      }
    }
    const needleFile = index % NEEDLE_EVERY === 0;
    // Needles always sit in source text, never in a binary-looking file a
    // search engine might skip by extension.
    if (needleFile) extension = "ts";
    const name = `${pick(WORDS)}-${index}.${extension}`;
    const needle = needleFile ? ` ${SYNTHETIC_NEEDLE}` : "";
    yield {
      path: parent.path ? `${parent.path}/${name}` : name,
      content: `synthetic ${index}${needle}\n`,
    };
  }
}

/**
 * Counts and a SHA-256 over every path and content, sorted by path, so a tree
 * measured from disk compares equal to the generated one. `visible*` counts
 * leave out files under the ignored folders, which a .gitignore-aware search
 * must skip.
 */
function summarize(files) {
  const entries = [...files].sort((a, b) =>
    a.path < b.path ? -1 : a.path > b.path ? 1 : 0,
  );
  const hash = createHash("sha256");
  const directories = new Set();
  const ignoredByRoot = Object.fromEntries(
    IGNORED_DIRECTORIES.map((name) => [name, 0]),
  );
  let needles = 0;
  let visibleNeedles = 0;
  let maxDepth = 0;
  let maxPathLength = 0;
  for (const file of entries) {
    hash.update(`${file.path}\0${file.content}\0`);
    const parts = file.path.split("/");
    maxDepth = Math.max(maxDepth, parts.length - 1);
    maxPathLength = Math.max(maxPathLength, file.path.length);
    for (let end = 1; end < parts.length; end += 1) {
      directories.add(parts.slice(0, end).join("/"));
    }
    const ignored = parts.length > 1 && parts[0] in ignoredByRoot;
    if (ignored) ignoredByRoot[parts[0]] += 1;
    if (file.content.includes(SYNTHETIC_NEEDLE)) {
      needles += 1;
      if (!ignored) visibleNeedles += 1;
    }
  }
  const ignoredFiles = Object.values(ignoredByRoot).reduce((a, b) => a + b, 0);
  return {
    files: entries.length,
    directories: directories.size,
    visibleFiles: entries.length - ignoredFiles,
    ignoredFiles,
    ignoredByRoot,
    needleFiles: needles,
    visibleNeedleFiles: visibleNeedles,
    maxDepth,
    maxPathLength,
    sha256: hash.digest("hex"),
  };
}

export function syntheticTreeManifest(options = {}) {
  return summarize(syntheticTree(options));
}

/** Written last, so a tree without it is known to be incomplete. */
export const COMPLETE_MARKER = ".synthetic-tree-complete.json";

/** Writes the tree into an empty or missing directory; never overwrites. */
export function writeSyntheticTree(target, options = {}) {
  mkdirSync(target, { recursive: true });
  if (readdirSync(target).length > 0)
    throw new Error(`synthetic-tree: ${target} is not empty`);
  const made = new Set();
  for (const file of syntheticTree(options)) {
    const absolute = path.join(target, ...file.path.split("/"));
    const parent = path.dirname(absolute);
    if (!made.has(parent)) {
      mkdirSync(parent, { recursive: true });
      made.add(parent);
    }
    writeFileSync(absolute, file.content, { flag: "wx" });
  }
  // The manifest is measured from disk, not recomputed from the generator.
  const manifest = measureSyntheticTree(target);
  writeFileSync(
    path.join(target, COMPLETE_MARKER),
    `${JSON.stringify({ ...options, ...manifest }, null, 2)}\n`,
    { flag: "wx" },
  );
  return manifest;
}

/** Summarizes a tree as it is on disk, skipping the completion marker. */
export function measureSyntheticTree(target) {
  const files = [];
  const pending = [""];
  while (pending.length > 0) {
    const relative = pending.pop();
    const absolute = path.join(target, ...relative.split("/").filter(Boolean));
    for (const entry of readdirSync(absolute, { withFileTypes: true })) {
      const child = relative ? `${relative}/${entry.name}` : entry.name;
      if (entry.isDirectory()) pending.push(child);
      else if (child !== COMPLETE_MARKER) {
        files.push({
          path: child,
          content: readFileSync(path.join(absolute, entry.name), "utf8"),
        });
      }
    }
  }
  return summarize(files);
}

export function parseArguments(argv) {
  const options = { seed: 1, count: DEFAULT_SYNTHETIC_FILES, out: null };
  for (let index = 0; index < argv.length; index += 2) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (!["--seed", "--count", "--out"].includes(flag))
      throw new Error(`synthetic-tree: unknown argument ${flag}`);
    if (value === undefined || value.startsWith("--") || value.trim() === "")
      throw new Error(`synthetic-tree: ${flag} needs a value`);
    if (flag === "--out") {
      options.out = value;
    } else {
      if (!/^\d+$/u.test(value))
        throw new Error(`synthetic-tree: ${flag} must be a whole number`);
      options[flag.slice(2)] = Number(value);
    }
  }
  return options;
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  try {
    const { out, ...options } = parseArguments(process.argv.slice(2));
    const manifest = out
      ? writeSyntheticTree(path.resolve(out), options)
      : syntheticTreeManifest(options);
    console.log(JSON.stringify({ ...options, ...manifest }, null, 2));
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
