import { constants } from "node:fs";
import { lstat, open, opendir } from "node:fs/promises";
import path from "node:path";

export const Q02_LIMITS = Object.freeze({
  files: 480,
  depth: 16,
  fileBytes: 64 * 1024,
  aggregateBytes: 2 * 1024 * 1024,
  visitedEntries: 1024,
  errors: 32,
});

export const Q02_POLICY_OWNERSHIP = Object.freeze([
  {
    rule: "isRecord",
    path: "src/domain/guards.ts",
    symbol: "isRecord",
    reason: "shared plain-record primitive",
  },
  {
    rule: "errorMessage",
    path: "src/domain/errors.ts",
    symbol: "errorMessage",
    reason: "shared base error extraction",
  },
  {
    rule: "boundedError",
    path: "src/domain/errors.ts",
    symbol: "boundedError",
    reason: "shared explicit-unit error bound",
  },
  {
    rule: "utf8ByteLength",
    path: "src/domain/text.ts",
    symbol: "utf8ByteLength",
    reason: "shared UTF-8 measurement",
  },
  {
    rule: "encoder",
    path: "src/domain/text.ts",
    symbol: "encoder",
    reason: "sole shared TextEncoder",
  },
  {
    rule: "coreImport",
    path: "src/ports/nativeInvoke.ts",
    symbol: "invokeNative",
    reason: "sole dynamic core invoke boundary",
  },
  {
    rule: "nonArrayRecord",
    path: "src/features/chatSearch/chatSearchDomain.ts",
    symbol: "isNonArrayRecord",
    reason: "Chat Search rejects arrays",
    discriminator: "plain record plus Array.isArray rejection",
  },
]);

const production = /\.(?:ts|tsx)$/u;
const ignored =
  /(?:\.(?:test|spec|fixture)|(?:\.test|\.spec|\.fixture))\.(?:ts|tsx)$/u;
const rules = [
  ["isRecord", /\b(?:function|const|let|var)\s+isRecord\b/u],
  ["errorMessage", /\b(?:function|const|let|var)\s+errorMessage\b/u],
  ["boundedError", /\b(?:function|const|let|var)\s+boundedError\b/u],
  ["utf8ByteLength", /\b(?:function|const|let|var)\s+utf8ByteLength\b/u],
  ["encoder", /new\s+TextEncoder\s*\(/u],
  ["coreImport", /@tauri-apps\/api\/core/u],
  ["nonArrayRecord", /\b(?:function|const|let|var)\s+isNonArrayRecord\b/u],
];

function sameIdentity(left, right) {
  return left.dev === right.dev && left.ino === right.ino;
}

function compareEntries(left, right) {
  if (left.entry.name < right.entry.name) return -1;
  if (left.entry.name > right.entry.name) return 1;
  return left.ordinal - right.ordinal;
}

function owned(rule, file) {
  return Q02_POLICY_OWNERSHIP.some(
    (entry) => entry.rule === rule && entry.path === file,
  );
}

async function readBoundedRegularFile(
  file,
  expected,
  maximumBytes,
  fileSystem,
) {
  const flags = constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0);
  let handle;
  try {
    handle = await fileSystem.open(file, flags);
  } catch {
    return null;
  }
  try {
    const before = await handle.stat();
    if (
      !before.isFile() ||
      !sameIdentity(before, expected) ||
      before.size !== expected.size ||
      before.size > maximumBytes
    ) {
      return null;
    }
    const bytes = Buffer.allocUnsafe(before.size + 1);
    const { bytesRead } = await handle.read(bytes, 0, bytes.length, 0);
    const after = await handle.stat();
    const pathname = await fileSystem.lstat(file);
    if (
      bytesRead !== before.size ||
      !sameIdentity(after, before) ||
      !pathname.isFile() ||
      !sameIdentity(pathname, expected) ||
      pathname.size !== expected.size
    ) {
      return null;
    }
    return bytes.subarray(0, bytesRead).toString("utf8");
  } finally {
    await handle.close();
  }
}

export async function q02SharedHelperPolicyErrors(root, options = {}) {
  const limits = { ...Q02_LIMITS, ...options.limits };
  const sourceDirectory = options.sourceDirectory ?? "src";
  const fileSystem = { lstat, open, opendir, ...options.fileSystem };
  const errors = [];
  let visitedEntries = 0;
  let files = 0;
  let aggregateBytes = 0;
  const fail = (message) => {
    if (errors.length < limits.errors) errors.push(message);
  };

  async function revalidateObservedDirectory(relative, expectedDirectory) {
    try {
      const current = await fileSystem.lstat(path.join(root, relative));
      if (
        current.isSymbolicLink() ||
        !current.isDirectory() ||
        !sameIdentity(current, expectedDirectory)
      ) {
        fail(`Q02 source directory identity changed: ${relative}`);
        return false;
      }
      return true;
    } catch {
      fail(`Q02 source directory identity changed: ${relative}`);
      return false;
    }
  }

  async function visit(relative, depth, expectedDirectory) {
    let current;
    try {
      current = await fileSystem.lstat(path.join(root, relative));
    } catch {
      fail(`Q02 source directory missing: ${relative}`);
      return false;
    }
    if (current.isSymbolicLink()) {
      fail(`Q02 source directory is a symlink: ${relative}`);
      return false;
    }
    if (!current.isDirectory()) {
      fail(`Q02 source entry is not a directory: ${relative}`);
      return false;
    }
    if (expectedDirectory && !sameIdentity(current, expectedDirectory)) {
      fail(`Q02 source directory identity changed: ${relative}`);
      return false;
    }
    if (depth > limits.depth) {
      fail(`Q02 depth limit ${limits.depth} exceeded at ${relative}`);
      return false;
    }
    const observedDirectory = current;

    let directory;
    try {
      directory = await fileSystem.opendir(path.join(root, relative));
    } catch {
      fail(`Q02 source directory cannot be opened: ${relative}`);
      return false;
    }
    const entries = [];
    let ordinal = 0;
    for await (const entry of directory) {
      visitedEntries += 1;
      if (visitedEntries > limits.visitedEntries) {
        fail(
          `Q02 visited-entry limit ${limits.visitedEntries} exceeded at ${path.posix.join(relative, entry.name)}`,
        );
        return false;
      }
      entries.push({ entry, ordinal });
      ordinal += 1;
    }
    entries.sort(compareEntries);
    if (!(await revalidateObservedDirectory(relative, observedDirectory)))
      return false;

    for (const { entry } of entries) {
      const child = path.posix.join(relative, entry.name);
      let metadata;
      try {
        metadata = await fileSystem.lstat(path.join(root, child));
      } catch {
        fail(`Q02 source entry disappeared: ${child}`);
        return false;
      }
      if (metadata.isSymbolicLink()) {
        fail(`Q02 source entry is a symlink: ${child}`);
        return false;
      }
      if (metadata.isDirectory()) {
        if (!(await visit(child, depth + 1, metadata))) return false;
        continue;
      }
      if (!metadata.isFile()) {
        fail(`Q02 non-regular source entry: ${child}`);
        return false;
      }
      if (!production.test(entry.name) || ignored.test(entry.name)) continue;

      files += 1;
      if (files > limits.files) {
        fail(`Q02 file limit ${limits.files} exceeded at ${child}`);
        return false;
      }
      if (metadata.size > limits.fileBytes) {
        fail(`Q02 per-file limit ${limits.fileBytes} exceeded at ${child}`);
        return false;
      }
      aggregateBytes += metadata.size;
      if (aggregateBytes > limits.aggregateBytes) {
        fail(
          `Q02 aggregate limit ${limits.aggregateBytes} exceeded at ${child}`,
        );
        return false;
      }
      const source = await readBoundedRegularFile(
        path.join(root, child),
        metadata,
        limits.fileBytes,
        fileSystem,
      );
      if (source === null) {
        fail(`Q02 bounded file identity/read failure: ${child}`);
        return false;
      }
      for (const [rule, expression] of rules) {
        if (expression.test(source) && !owned(rule, child)) {
          fail(`Q02 forbidden ${rule} at ${child}`);
        }
      }
    }
    if (!(await revalidateObservedDirectory(relative, observedDirectory)))
      return false;
    return true;
  }

  await visit(sourceDirectory, 0);
  return errors;
}
