import { createHash } from "node:crypto";
import { lstat, open } from "node:fs/promises";

const markers = Object.freeze({
  source: Buffer.from("__TAURI_BUNDLE_TYPE_VAR_UNK"),
  msi: Buffer.from("__TAURI_BUNDLE_TYPE_VAR_MSI"),
  nsis: Buffer.from("__TAURI_BUNDLE_TYPE_VAR_NSS"),
});

function sameIdentity(left, right) {
  return (
    left.dev === right.dev &&
    left.ino === right.ino &&
    left.size === right.size &&
    left.mtimeNs === right.mtimeNs
  );
}

async function openStableFile(filePath, maximumBytes) {
  const pathIdentity = await lstat(filePath, { bigint: true });
  if (!pathIdentity.isFile() || pathIdentity.isSymbolicLink()) {
    throw new Error("package executable must be one regular non-linked file");
  }
  if (pathIdentity.size < 1n || pathIdentity.size > BigInt(maximumBytes)) {
    throw new Error("package executable exceeds its byte limit");
  }
  const handle = await open(filePath, "r");
  const openedIdentity = await handle.stat({ bigint: true });
  if (!sameIdentity(pathIdentity, openedIdentity)) {
    await handle.close();
    throw new Error("package executable identity changed while opening");
  }
  return {
    bytes: Number(openedIdentity.size),
    filePath,
    handle,
    identity: openedIdentity,
  };
}

async function assertStable(value) {
  const [openedIdentity, pathIdentity] = await Promise.all([
    value.handle.stat({ bigint: true }),
    lstat(value.filePath, { bigint: true }),
  ]);
  if (
    pathIdentity.isSymbolicLink() ||
    !sameIdentity(value.identity, openedIdentity) ||
    !sameIdentity(value.identity, pathIdentity)
  ) {
    throw new Error("package executable identity changed during inspection");
  }
}

async function digestRanges(value, ranges) {
  const digest = createHash("sha256");
  for (const range of ranges) {
    if (Buffer.isBuffer(range)) {
      digest.update(range);
      continue;
    }
    if (range.start > range.end) continue;
    for await (const chunk of value.handle.createReadStream({
      autoClose: false,
      end: range.end,
      highWaterMark: 64 * 1024,
      start: range.start,
    })) {
      digest.update(chunk);
    }
  }
  return digest.digest("hex");
}

async function tokenOffsets(value, token) {
  const offsets = [];
  let carry = Buffer.alloc(0);
  let consumed = 0;
  let lastOffset = -1;
  for await (const chunk of value.handle.createReadStream({
    autoClose: false,
    end: value.bytes - 1,
    highWaterMark: 64 * 1024,
    start: 0,
  })) {
    const joined = Buffer.concat([carry, chunk]);
    const joinedOffset = consumed - carry.byteLength;
    let cursor = 0;
    while ((cursor = joined.indexOf(token, cursor)) >= 0) {
      const absoluteOffset = joinedOffset + cursor;
      if (absoluteOffset > lastOffset) {
        offsets.push(absoluteOffset);
        lastOffset = absoluteOffset;
      }
      cursor += 1;
    }
    consumed += chunk.byteLength;
    carry = joined.subarray(
      Math.max(0, joined.byteLength - (token.byteLength - 1)),
    );
  }
  return offsets;
}

export async function verifyTauriBundleExecutable({
  sourceExecutable,
  installedExecutable,
  installer,
  maximumBytes,
}) {
  const targetMarker = markers[installer];
  if (!targetMarker) throw new Error("package installer is invalid");
  const source = await openStableFile(sourceExecutable, maximumBytes);
  const installed = await openStableFile(installedExecutable, maximumBytes);
  try {
    if (source.bytes !== installed.bytes) {
      throw new Error("installed executable size differs from its source");
    }
    const [
      sourceOffsets,
      sourceTargetOffsets,
      installedOffsets,
      targetOffsets,
    ] = await Promise.all([
      tokenOffsets(source, markers.source),
      tokenOffsets(source, targetMarker),
      tokenOffsets(installed, markers.source),
      tokenOffsets(installed, targetMarker),
    ]);
    const expectedTargetOffsets = [
      ...sourceTargetOffsets,
      sourceOffsets[0],
    ].sort((left, right) => left - right);
    if (
      sourceOffsets.length !== 1 ||
      installedOffsets.length !== 0 ||
      JSON.stringify(targetOffsets) !== JSON.stringify(expectedTargetOffsets)
    ) {
      throw new Error("Tauri bundle-type marker projection is invalid");
    }
    const markerOffset = sourceOffsets[0];
    const [sourceSha256, expectedSha256, installedSha256] = await Promise.all([
      digestRanges(source, [{ start: 0, end: source.bytes - 1 }]),
      digestRanges(source, [
        { start: 0, end: markerOffset - 1 },
        targetMarker,
        {
          start: markerOffset + markers.source.byteLength,
          end: source.bytes - 1,
        },
      ]),
      digestRanges(installed, [{ start: 0, end: installed.bytes - 1 }]),
    ]);
    if (expectedSha256 !== installedSha256) {
      throw new Error(
        "installed executable contains changes beyond Tauri's bundle-type projection",
      );
    }
    await Promise.all([assertStable(source), assertStable(installed)]);
    return {
      algorithm: "tauri-bundle-type-patch-v1",
      sourceMarkerOffset: markerOffset,
      sourceMarkerOccurrences: sourceOffsets.length,
      targetMarkerOccurrences: targetOffsets.length,
      sourceExecutableSha256: sourceSha256,
      expectedInstalledExecutableSha256: expectedSha256,
      installedExecutableSha256: installedSha256,
      installedExecutableBytes: installed.bytes,
    };
  } finally {
    await Promise.all([source.handle.close(), installed.handle.close()]);
  }
}
