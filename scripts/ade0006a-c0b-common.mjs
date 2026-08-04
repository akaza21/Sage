import { createHash } from "node:crypto";
import {
  closeSync,
  constants,
  fstatSync,
  lstatSync,
  openSync,
  readSync,
} from "node:fs";

export const SHA256 = /^[0-9a-f]{64}$/;
export const HEX_ID = /^(?:[0-9a-f]{40}|[0-9a-f]{64})$/;
export const SLUG = /^[a-z0-9][a-z0-9._-]{0,127}$/;

export function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

export function stableJson(value) {
  if (Array.isArray(value)) return `[${value.map(stableJson).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${stableJson(value[key])}`)
      .join(",")}}`;
  }
  return JSON.stringify(value);
}

export function recordDigest(value) {
  return sha256(stableJson(value));
}

export function sameFileIdentity(before, opened) {
  return before.dev === opened.dev && before.ino === opened.ino;
}

export function readBoundedFile(filePath, maximumBytes, label) {
  const before = lstatSync(filePath);
  if (!before.isFile() || before.isSymbolicLink()) {
    throw new Error(`${label} must be a regular non-symlink file`);
  }
  if (before.size < 1 || before.size > maximumBytes) {
    throw new Error(`${label} must contain 1..${maximumBytes} bytes`);
  }
  const noFollow =
    process.platform === "win32" || !Number.isInteger(constants.O_NOFOLLOW)
      ? 0
      : constants.O_NOFOLLOW;
  const descriptor = openSync(filePath, constants.O_RDONLY | noFollow);
  try {
    const opened = fstatSync(descriptor);
    if (!opened.isFile() || !sameFileIdentity(before, opened)) {
      throw new Error(`${label} was replaced before it was opened`);
    }
    if (opened.size < 1 || opened.size > maximumBytes) {
      throw new Error(`${label} must contain 1..${maximumBytes} bytes`);
    }
    const bytes = Buffer.allocUnsafe(opened.size);
    let offset = 0;
    while (offset < bytes.length) {
      const count = readSync(
        descriptor,
        bytes,
        offset,
        bytes.length - offset,
        offset,
      );
      if (count === 0) throw new Error(`${label} changed while being read`);
      offset += count;
    }
    const extra = Buffer.allocUnsafe(1);
    if (readSync(descriptor, extra, 0, 1, offset) !== 0) {
      throw new Error(`${label} grew beyond its admitted size`);
    }
    const after = fstatSync(descriptor);
    if (
      after.size !== opened.size ||
      after.mtimeMs !== opened.mtimeMs ||
      after.ino !== opened.ino ||
      after.dev !== opened.dev
    ) {
      throw new Error(`${label} identity changed while being read`);
    }
    return bytes;
  } finally {
    closeSync(descriptor);
  }
}

export function validateJsonShape(value, limits, label) {
  const stack = [{ value, depth: 1 }];
  let nodes = 0;
  while (stack.length > 0) {
    const current = stack.pop();
    nodes += 1;
    if (nodes > limits.jsonNodes) {
      throw new Error(`${label} exceeds ${limits.jsonNodes} JSON nodes`);
    }
    if (current.depth > limits.jsonDepth) {
      throw new Error(`${label} exceeds JSON depth ${limits.jsonDepth}`);
    }
    if (Array.isArray(current.value)) {
      if (current.value.length > limits.arrayEntries) {
        throw new Error(
          `${label} array exceeds ${limits.arrayEntries} entries`,
        );
      }
      for (const entry of current.value) {
        stack.push({ value: entry, depth: current.depth + 1 });
      }
    } else if (current.value && typeof current.value === "object") {
      const entries = Object.entries(current.value);
      if (entries.length > limits.objectMembers) {
        throw new Error(
          `${label} object exceeds ${limits.objectMembers} members`,
        );
      }
      for (const [key, entry] of entries) {
        if (Buffer.byteLength(key) > limits.decodedKeyBytes) {
          throw new Error(
            `${label} key exceeds ${limits.decodedKeyBytes} bytes`,
          );
        }
        stack.push({ value: entry, depth: current.depth + 1 });
      }
    }
  }
}

export function parseBoundedJson(filePath, maximumBytes, limits, label) {
  const bytes = readBoundedFile(filePath, maximumBytes, label);
  const value = JSON.parse(bytes);
  validateJsonShape(value, limits, label);
  return { value, bytes: bytes.byteLength, sha256: sha256(bytes) };
}

export function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

export function exactKeys(errors, value, keys, label) {
  if (!isRecord(value)) {
    errors.push(`${label} must be an object`);
    return false;
  }
  const actual = Object.keys(value).sort().join("\0");
  const expected = [...keys].sort().join("\0");
  if (actual !== expected)
    errors.push(`${label} has missing or unknown fields`);
  return true;
}

export function safeInteger(errors, value, label, minimum, maximum) {
  if (!Number.isSafeInteger(value) || value < minimum || value > maximum) {
    errors.push(`${label} must be an integer in ${minimum}..${maximum}`);
  }
}

export function safeSlug(errors, value, label) {
  if (typeof value !== "string" || !SLUG.test(value)) {
    errors.push(`${label} must be a bounded sanitized label`);
  }
}

const unsafeKey =
  /(^|_)(argv|environment|workingdirectory|absolutepath|secret|token|commandline|rawframe|rawpayload)(_|$)/i;
const thresholdKey = /(budget|ceiling|threshold)/i;
const embeddedPath =
  /(?:file:(?:\/{2,3}|\\\\)|(?:^|[\s="'([{])(?:[a-z]:[\\/]|\\\?\?\\[a-z]:[\\/]|\\\\(?:\?\\|\.\\|[^\\])|\\[a-z0-9._-]+(?:[\\/][a-z0-9._-]+)*(?=$|[\s="')\]}])|\/[a-z0-9._-]+(?:\/[a-z0-9._-]+)*(?=$|[\s="')\]}])))/i;
const embeddedSecret =
  /(?:bearer\s+[a-z0-9._-]+|(?:api[-_ ]?key|password|secret|token)\s*[=:]\s*\S+)/i;
const embeddedCommand =
  /(?:^|\s)(?:(?:cmd|powershell|bash|zsh|node|npm|cargo|git|python(?:3(?:\.\d+)?)?)(?:\.exe|\.cmd)?\s+(?:--?|[./\\]|[a-z0-9_<])|sage(?:\.exe)?\s+(?:--?|[./\\]))/i;

export function inspectSanitized(value, errors, label, trustedKeys = []) {
  const trusted = new Set(trustedKeys);
  const stack = [{ value, label }];
  while (stack.length > 0) {
    const current = stack.pop();
    if (typeof current.value === "string") {
      if (Buffer.byteLength(current.value) > 8192) {
        errors.push(`${current.label} exceeds 8192 sanitized bytes`);
      }
      if (embeddedPath.test(current.value)) {
        errors.push(`${current.label} contains an embedded absolute path`);
      }
      if (embeddedSecret.test(current.value)) {
        errors.push(`${current.label} contains secret-like material`);
      }
      if (embeddedCommand.test(current.value)) {
        errors.push(`${current.label} contains a command line`);
      }
    } else if (Array.isArray(current.value)) {
      current.value.forEach((entry, index) =>
        stack.push({ value: entry, label: `${current.label}[${index}]` }),
      );
    } else if (isRecord(current.value)) {
      for (const [key, entry] of Object.entries(current.value)) {
        if (unsafeKey.test(key))
          errors.push(`${current.label}.${key} is forbidden`);
        if (thresholdKey.test(key) && !trusted.has(key)) {
          errors.push(`${current.label}.${key} defines an untrusted threshold`);
        }
        stack.push({ value: entry, label: `${current.label}.${key}` });
      }
    }
  }
}

export function sameValue(errors, actual, expected, label) {
  if (stableJson(actual) !== stableJson(expected)) {
    errors.push(`${label} does not match trusted provenance`);
  }
}
