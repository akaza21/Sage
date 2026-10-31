import { readFileSync } from "node:fs";
import path from "node:path";
import { gzipSync } from "node:zlib";

const workbench = "src/workbench/Workbench.tsx";
// The Editor's tab strip loads lazily with every Editor view.
const tabStrip = "src/workbench/TabStrip.tsx";
const MAX_MANIFEST_RECORDS = 4096;
const MAX_MANIFEST_IMPORTS = 16384;
export const WEB_BUNDLE_PATH_ROOTS = {
  initial: ["index.html"],
  agentOneChat: [
    "index.html",
    workbench,
    "src/workbench/AgentWorkbenchBody.tsx",
    "src/features/chat/ChatSurface.tsx",
    "src/features/chat/acp/AcpChatPresentation.tsx",
  ],
  editorShell: ["index.html", workbench, tabStrip],
  editorDiff: [
    "index.html",
    workbench,
    tabStrip,
    "src/features/diff/DiffSurface.tsx",
  ],
  editorSearch: [
    "index.html",
    workbench,
    tabStrip,
    "src/features/search/SearchSurface.tsx",
  ],
  editorFile: [
    "index.html",
    workbench,
    tabStrip,
    "src/features/files/FileSurface.tsx",
    "src/features/files/CodeMirrorEditorRuntime.ts",
  ],
};

function fail(message) {
  throw new Error(`web-bundle: ${message}`);
}

export function validateWebBundleManifest(manifest) {
  if (!manifest || typeof manifest !== "object" || Array.isArray(manifest))
    fail("manifest is malformed");
  const keys = Object.keys(manifest);
  if (keys.length > MAX_MANIFEST_RECORDS)
    fail("manifest record limit exceeded");
  let importCount = 0;
  for (const key of keys) {
    const record = manifest[key];
    if (!record || typeof record !== "object" || Array.isArray(record))
      fail(`manifest import is missing: ${key}`);
    for (const field of ["imports", "dynamicImports"]) {
      const imports = record[field] ?? [];
      if (!Array.isArray(imports))
        fail(`manifest import list is malformed: ${key}`);
      importCount += imports.length;
      if (importCount > MAX_MANIFEST_IMPORTS)
        fail("manifest import limit exceeded");
      if (imports.some((value) => typeof value !== "string"))
        fail(`manifest import list is malformed: ${key}`);
    }
  }
  return keys;
}

export function measureWebBundlePaths(root, manifest) {
  const keys = validateWebBundleManifest(manifest);
  const reachable = new Set();
  const pending = ["index.html"];
  while (pending.length > 0) {
    const key = pending.pop();
    if (reachable.has(key)) continue;
    const record = manifest[key];
    if (!record || typeof record !== "object")
      fail(`manifest import is missing: ${key}`);
    reachable.add(key);
    pending.push(...(record.imports ?? []), ...(record.dynamicImports ?? []));
  }
  function resolveRoot(key) {
    if (manifest[key]) return key;
    const names =
      key === workbench
        ? ["workbench-deps"]
        : key === "src/workbench/AgentWorkbenchBody.tsx"
          ? ["agent-workbench", "workbench-deps"]
          : key === "src/features/chat/ChatSurface.tsx"
            ? ["agent-workbench", "workbench-deps"]
            : key === "src/features/chat/acp/AcpChatPresentation.tsx"
              ? ["chat-ui", "agent-workbench"]
              : key === "src/features/files/CodeMirrorEditorRuntime.ts"
                ? ["codemirror-deps"]
                : [];
    for (const name of names) {
      const matches = keys.filter(
        (candidate) =>
          manifest[candidate].name === name &&
          manifest[candidate].file?.endsWith(".js"),
      );
      if (matches.length > 1) fail(`path root is ambiguous: ${key} (${name})`);
      if (matches.length === 1) return matches[0];
    }
    fail(`path root is missing: ${key}`);
  }
  const measurements = new Map();
  function measure(file) {
    if (measurements.has(file)) return measurements.get(file);
    if (
      typeof file !== "string" ||
      !file.startsWith("assets/") ||
      file.includes("..") ||
      path.isAbsolute(file)
    )
      fail("manifest contains an escaping asset path");
    let contents;
    try {
      contents = readFileSync(path.join(root, "dist", file));
    } catch {
      fail(`asset is missing: ${file}`);
    }
    if (!file.endsWith(".js") || contents.length === 0)
      fail(`path asset is empty or not JavaScript: ${file}`);
    const result = {
      gzipBytes: gzipSync(contents).length,
      rawBytes: contents.length,
    };
    measurements.set(file, result);
    return result;
  }
  return Object.fromEntries(
    Object.entries(WEB_BUNDLE_PATH_ROOTS).map(([name, roots]) => {
      const visited = new Set();
      const files = new Set();
      const remaining = roots.map(resolveRoot);
      while (remaining.length > 0) {
        const key = remaining.pop();
        if (visited.has(key)) continue;
        if (!reachable.has(key))
          fail(`path root/import is unreachable: ${key}`);
        visited.add(key);
        const record = manifest[key];
        measure(record.file);
        files.add(record.file);
        remaining.push(...(record.imports ?? []));
      }
      const totals = [...files].reduce(
        (sum, file) => {
          const size = measure(file);
          return {
            gzipBytes: sum.gzipBytes + size.gzipBytes,
            rawBytes: sum.rawBytes + size.rawBytes,
          };
        },
        { gzipBytes: 0, rawBytes: 0 },
      );
      return [name, { ...totals, files: [...files].sort() }];
    }),
  );
}

export function verifyWebBundlePathBudgets(paths, limits) {
  const exceeded = [];
  for (const name of Object.keys(WEB_BUNDLE_PATH_ROOTS)) {
    const limit = limits?.[name];
    if (!Number.isInteger(limit) || limit < 1)
      fail(`path limit ${name} is missing or invalid`);
    if (paths[name].gzipBytes > limit)
      exceeded.push(
        `${name} static closure gzip ${paths[name].gzipBytes} exceeds ${limit}`,
      );
  }
  if (exceeded.length) fail(exceeded.join("; "));
}
