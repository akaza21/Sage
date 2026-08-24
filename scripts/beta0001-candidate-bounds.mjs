import { lstat, open } from "node:fs/promises";
import path from "node:path";

function reject(code) {
  const error = new Error(code);
  error.code = code;
  throw error;
}

function sameIdentity(left, right) {
  return (
    left.dev === right.dev &&
    left.ino === right.ino &&
    left.size === right.size &&
    left.nlink === right.nlink &&
    left.mtimeNs === right.mtimeNs
  );
}

function isReservedWindowsName(value) {
  return /^(?:con|prn|aux|nul|com[1-9¹²³]|lpt[1-9¹²³])(?:\.|$)/iu.test(value);
}

function containsInvalidWindowsCharacter(value) {
  return [...value].some(
    (character) =>
      character.codePointAt(0) <= 0x1f || '<>:"|?*'.includes(character),
  );
}

export function betaCandidateRelativePathComponents(value, limits) {
  const maximumBytes = limits?.relativePathBytesMaximum ?? limits?.maximumBytes;
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    !Number.isSafeInteger(maximumBytes) ||
    maximumBytes < 1 ||
    Buffer.byteLength(value, "utf8") > maximumBytes ||
    value.includes("\\") ||
    value.startsWith("/") ||
    path.win32.isAbsolute(value)
  )
    reject("evidence-path-invalid");
  const components = value.split("/");
  if (
    components.some(
      (component) =>
        component.length === 0 ||
        component === "." ||
        component === ".." ||
        component.endsWith(".") ||
        component.endsWith(" ") ||
        isReservedWindowsName(component) ||
        containsInvalidWindowsCharacter(component),
    )
  )
    reject("evidence-path-invalid");
  return components;
}

export function inspectBetaCandidateJson(value, limits) {
  const stack = [{ value, depth: 1 }];
  let nodes = 0;
  while (stack.length > 0) {
    const current = stack.pop();
    nodes += 1;
    if (nodes > limits.jsonNodesMaximum) reject("json-nodes");
    if (current.depth > limits.jsonDepthMaximum) reject("json-depth");
    if (typeof current.value === "string") {
      if (
        Buffer.byteLength(current.value, "utf8") > limits.stringBytesMaximum
      ) {
        reject("json-string");
      }
    } else if (Array.isArray(current.value)) {
      for (const child of current.value)
        stack.push({ value: child, depth: current.depth + 1 });
    } else if (current.value && typeof current.value === "object") {
      for (const [key, child] of Object.entries(current.value)) {
        if (Buffer.byteLength(key, "utf8") > limits.stringBytesMaximum)
          reject("json-string");
        stack.push({ value: child, depth: current.depth + 1 });
      }
    }
  }
}

function assertPlain(metadata) {
  if (
    !metadata.isFile() ||
    metadata.isSymbolicLink() ||
    metadata.nlink !== 1n
  ) {
    reject("evidence-link");
  }
}

export async function readBoundedPlainEvidenceFile({
  root,
  id,
  maximumBytes,
  onAfterOpen,
}) {
  const components = betaCandidateRelativePathComponents(id, {
    maximumBytes: 512,
  });
  if (
    components.length !== 1 ||
    !/^[a-z0-9]+(?:[.-][a-z0-9]+)*$/u.test(components[0])
  ) {
    reject("evidence-path-invalid");
  }
  const [component] = components;
  if (!Number.isSafeInteger(maximumBytes) || maximumBytes < 0)
    reject("evidence-oversize");
  let rootMetadata;
  try {
    rootMetadata = await lstat(root, { bigint: true });
  } catch {
    reject("evidence-path-invalid");
  }
  if (
    !rootMetadata.isDirectory() ||
    rootMetadata.isSymbolicLink() ||
    rootMetadata.nlink !== 1n
  ) {
    reject("evidence-link");
  }
  const file = path.join(root, `${component}.json`);
  const before = await lstat(file, { bigint: true });
  assertPlain(before);
  if (before.size > BigInt(maximumBytes)) reject("evidence-oversize");
  let handle;
  try {
    handle = await open(file, "r");
  } catch {
    reject("evidence-identity-changed");
  }
  try {
    if (onAfterOpen) await onAfterOpen();
    const opened = await handle.stat({ bigint: true });
    if (!sameIdentity(before, opened)) reject("evidence-identity-changed");
    const buffer = Buffer.allocUnsafe(maximumBytes + 1);
    let offset = 0;
    while (offset < buffer.length) {
      const { bytesRead } = await handle.read(
        buffer,
        offset,
        buffer.length - offset,
        null,
      );
      if (bytesRead === 0) break;
      offset += bytesRead;
    }
    if (offset > maximumBytes) reject("evidence-oversize");
    const afterRead = await handle.stat({ bigint: true });
    const afterPath = await lstat(file, { bigint: true });
    if (!sameIdentity(opened, afterRead) || !sameIdentity(before, afterPath)) {
      reject("evidence-identity-changed");
    }
    return buffer.subarray(0, offset);
  } finally {
    await handle.close();
  }
}
