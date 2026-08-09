import { createHash } from "node:crypto";
import { constants } from "node:fs";
import { lstat, open, unlink } from "node:fs/promises";
import path from "node:path";

function sameIdentity(left, right) {
  return (
    left.dev === right.dev &&
    left.ino === right.ino &&
    left.nlink === right.nlink &&
    left.size === right.size &&
    left.mtimeNs === right.mtimeNs
  );
}

function validateSource(identity, maximumBytes, rejectHardLinks) {
  if (
    !identity.isFile() ||
    identity.isSymbolicLink() ||
    (rejectHardLinks && identity.nlink !== 1n)
  ) {
    throw new Error("package artifact must be one regular non-linked file");
  }
  if (identity.size < 1n || identity.size > BigInt(maximumBytes)) {
    throw new Error(`package artifact must contain 1..${maximumBytes} bytes`);
  }
}

async function writeExactly(handle, bytes, position) {
  let offset = 0;
  while (offset < bytes.byteLength) {
    const result = await handle.write(
      bytes,
      offset,
      bytes.byteLength - offset,
      position + offset,
    );
    if (result.bytesWritten < 1) {
      throw new Error("package artifact snapshot write stopped early");
    }
    offset += result.bytesWritten;
  }
}

export async function snapshotPackageArtifact({
  sourcePath,
  destinationPath,
  maximumBytes,
  rejectHardLinks = true,
}) {
  if (
    !path.isAbsolute(sourcePath) ||
    !path.isAbsolute(destinationPath) ||
    path.resolve(sourcePath) === path.resolve(destinationPath) ||
    !Number.isSafeInteger(maximumBytes) ||
    maximumBytes < 1
  ) {
    throw new Error("package artifact snapshot request is invalid");
  }
  const before = await lstat(sourcePath, { bigint: true });
  validateSource(before, maximumBytes, rejectHardLinks);
  const noFollow =
    process.platform === "win32" || !Number.isInteger(constants.O_NOFOLLOW)
      ? 0
      : constants.O_NOFOLLOW;
  const source = await open(sourcePath, constants.O_RDONLY | noFollow);
  let destination;
  let destinationCreated = false;
  try {
    const opened = await source.stat({ bigint: true });
    validateSource(opened, maximumBytes, rejectHardLinks);
    if (!sameIdentity(before, opened)) {
      throw new Error("package artifact identity changed while opening");
    }
    destination = await open(destinationPath, "wx", 0o600);
    destinationCreated = true;
    const digest = createHash("sha256");
    const buffer = Buffer.allocUnsafe(64 * 1024);
    let position = 0;
    while (position < Number(opened.size)) {
      const length = Math.min(
        buffer.byteLength,
        Number(opened.size) - position,
      );
      const result = await source.read(buffer, 0, length, position);
      if (result.bytesRead < 1) {
        throw new Error("package artifact ended before its admitted size");
      }
      const chunk = buffer.subarray(0, result.bytesRead);
      digest.update(chunk);
      await writeExactly(destination, chunk, position);
      position += result.bytesRead;
    }
    const extra = Buffer.allocUnsafe(1);
    if ((await source.read(extra, 0, 1, position)).bytesRead !== 0) {
      throw new Error("package artifact grew beyond its admitted size");
    }
    await destination.sync();
    const [
      sourceAfter,
      sourcePathAfter,
      destinationAfter,
      destinationPathAfter,
    ] = await Promise.all([
      source.stat({ bigint: true }),
      lstat(sourcePath, { bigint: true }),
      destination.stat({ bigint: true }),
      lstat(destinationPath, { bigint: true }),
    ]);
    if (
      sourcePathAfter.isSymbolicLink() ||
      !sameIdentity(opened, sourceAfter) ||
      !sameIdentity(opened, sourcePathAfter)
    ) {
      throw new Error("package artifact identity changed while snapshotting");
    }
    if (
      !destinationAfter.isFile() ||
      destinationPathAfter.isSymbolicLink() ||
      !sameIdentity(destinationAfter, destinationPathAfter) ||
      destinationAfter.nlink !== 1n ||
      destinationAfter.size !== opened.size
    ) {
      throw new Error("package artifact snapshot identity is invalid");
    }
    return {
      path: destinationPath,
      sha256: digest.digest("hex"),
      bytes: position,
    };
  } catch (error) {
    if (destination) await destination.close().catch(() => {});
    destination = undefined;
    if (destinationCreated) await unlink(destinationPath).catch(() => {});
    throw error;
  } finally {
    if (destination) await destination.close();
    await source.close();
  }
}
