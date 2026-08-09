import { createHash } from "node:crypto";
import {
  lstat,
  mkdir,
  open,
  opendir,
  realpath,
  writeFile,
} from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { recordDigest, sha256 } from "./ade0006a-c0b-common.mjs";
import {
  defaultWorkspaceRoot,
  loadFrozenContracts,
} from "./ade0006a-c0b-contracts.mjs";
import { mapMsiInstalledMembers } from "./ade0006a-package-msi.mjs";

const scriptPath = fileURLToPath(import.meta.url);

function within(child, parent) {
  const relative = path.relative(parent, child);
  return (
    relative === "" ||
    (!relative.startsWith("..") && !path.isAbsolute(relative))
  );
}

export async function hashBoundedFile(
  filePath,
  maximumBytes,
  { rejectHardLinks = false } = {},
) {
  const before = await lstat(filePath, { bigint: true });
  if (
    !before.isFile() ||
    before.isSymbolicLink() ||
    (rejectHardLinks && before.nlink > 1n)
  ) {
    throw new Error("package member must be one regular non-linked file");
  }
  if (before.size < 0n || before.size > BigInt(maximumBytes)) {
    throw new Error("package member exceeds its byte limit");
  }
  const handle = await open(filePath, "r");
  try {
    const opened = await handle.stat({ bigint: true });
    if (
      before.dev !== opened.dev ||
      before.ino !== opened.ino ||
      before.size !== opened.size ||
      before.mtimeNs !== opened.mtimeNs
    ) {
      throw new Error("package member identity changed while opening");
    }
    const digest = createHash("sha256");
    let bytes = 0;
    for await (const chunk of handle.createReadStream({
      autoClose: false,
      highWaterMark: 64 * 1024,
    })) {
      bytes += chunk.byteLength;
      if (bytes > maximumBytes)
        throw new Error("package member grew past its byte limit");
      digest.update(chunk);
    }
    const [after, pathAfter] = await Promise.all([
      handle.stat({ bigint: true }),
      lstat(filePath, { bigint: true }),
    ]);
    if (
      pathAfter.isSymbolicLink() ||
      opened.dev !== after.dev ||
      opened.ino !== after.ino ||
      opened.size !== after.size ||
      opened.mtimeNs !== after.mtimeNs ||
      opened.dev !== pathAfter.dev ||
      opened.ino !== pathAfter.ino ||
      opened.size !== pathAfter.size ||
      opened.mtimeNs !== pathAfter.mtimeNs ||
      BigInt(bytes) !== opened.size
    ) {
      throw new Error("package member identity changed while hashing");
    }
    return { bytes, sha256: digest.digest("hex") };
  } finally {
    await handle.close();
  }
}

function compareUtf8(left, right) {
  return Buffer.compare(
    Buffer.from(left.relativePath),
    Buffer.from(right.relativePath),
  );
}

function validateRelativePath(relativePath, maximumBytes) {
  if (
    path.isAbsolute(relativePath) ||
    relativePath.includes("\\") ||
    relativePath
      .split("/")
      .some(
        (segment) => segment === "" || segment === "." || segment === "..",
      ) ||
    Buffer.byteLength(relativePath) > maximumBytes
  ) {
    throw new Error("package member path is invalid or exceeds its byte limit");
  }
}

export async function inspectBoundedTreeMembers(extractedRoot, bounds) {
  const resolvedRoot = await realpath(extractedRoot);
  const rootStat = await lstat(resolvedRoot);
  if (!rootStat.isDirectory() || rootStat.isSymbolicLink()) {
    throw new Error("extracted root must be a regular directory");
  }
  const pending = [{ absolutePath: resolvedRoot, segments: [] }];
  const records = [];
  const foldedPaths = new Set();
  let regularFileBytes = 0;
  while (pending.length > 0) {
    const current = pending.pop();
    const directory = await opendir(current.absolutePath, { bufferSize: 32 });
    for await (const entry of directory) {
      const segments = [...current.segments, entry.name];
      const relativePath = segments.join("/");
      validateRelativePath(relativePath, bounds.pathUtf8Bytes);
      const folded = relativePath.toLocaleLowerCase("en-US");
      if (foldedPaths.has(folded))
        throw new Error("package member path is duplicated");
      foldedPaths.add(folded);
      if (records.length >= bounds.membersPerInstaller) {
        throw new Error("package member count exceeds its bound");
      }
      const absolutePath = path.join(current.absolutePath, entry.name);
      const item = await lstat(absolutePath, { bigint: true });
      if (entry.isSymbolicLink() || item.isSymbolicLink()) {
        throw new Error(
          "package member cannot be a reparse point or symbolic link",
        );
      }
      const resolved = await realpath(absolutePath);
      if (!within(resolved, resolvedRoot)) {
        throw new Error("package member escapes the extracted root");
      }
      if (item.isDirectory()) {
        records.push({ relativePath, type: "directory" });
        pending.push({ absolutePath, segments });
      } else if (item.isFile()) {
        const file = await hashBoundedFile(
          absolutePath,
          bounds.extractedBytesPerInstaller,
          { rejectHardLinks: true },
        );
        regularFileBytes += file.bytes;
        if (regularFileBytes > bounds.extractedBytesPerInstaller) {
          throw new Error("extracted regular-file bytes exceed their bound");
        }
        records.push({ relativePath, type: "regularFile", ...file });
      } else {
        throw new Error("package member type is forbidden");
      }
    }
  }
  records.sort(compareUtf8);
  return records.map((record, index) => ({ ordinal: index + 1, ...record }));
}

function finalizeRecords(records) {
  const values = records.map((record) => {
    const value = { ...record };
    delete value.ordinal;
    return value;
  });
  values.sort(compareUtf8);
  return values.map((record, index) => ({ ordinal: index + 1, ...record }));
}

function mapNsisInstalledMembers(extractedRecords, projection) {
  const metadataRoots = new Set(projection.extractorMetadataRoots);
  const records = extractedRecords.filter((record) => {
    const root = record.relativePath.split("/", 1)[0];
    if (metadataRoots.has(root)) return false;
    if (root.startsWith("$"))
      throw new Error("NSIS extraction contains an unknown variable root");
    return true;
  });
  return {
    records,
    installedExecutableSourceRelativePath:
      projection.executableLogicalRelativePath,
    mapping: {
      method: projection.method,
      metadataSha256: null,
      mappedFileCount: records.filter((record) => record.type === "regularFile")
        .length,
      sha256: recordDigest(
        records.map((record) => ({ relativePath: record.relativePath })),
      ),
    },
  };
}

async function writeShards(
  records,
  outputDirectory,
  participant,
  installer,
  bounds,
) {
  await mkdir(outputDirectory, { recursive: false });
  const shards = [];
  for (
    let offset = 0;
    offset < records.length;
    offset += bounds.memberRecordsPerShard
  ) {
    if (shards.length >= bounds.memberManifestShardsPerInstaller) {
      throw new Error("package member manifest exceeds its shard bound");
    }
    const ordinal = shards.length + 1;
    const shardRecords = records.slice(
      offset,
      offset + bounds.memberRecordsPerShard,
    );
    const document = {
      schemaVersion: 1,
      participant,
      installer,
      shardOrdinal: ordinal,
      firstRecordOrdinal: offset + 1,
      records: shardRecords,
    };
    const bytes = Buffer.from(JSON.stringify(document));
    const filename = `members-${String(ordinal).padStart(4, "0")}.json`;
    await writeFile(path.join(outputDirectory, filename), bytes, {
      flag: "wx",
    });
    shards.push({
      ordinal,
      firstRecordOrdinal: offset + 1,
      recordCount: shardRecords.length,
      bytes: bytes.byteLength,
      sha256: sha256(bytes),
    });
  }
  const bytes = shards.reduce((total, shard) => total + shard.bytes, 0);
  if (bytes > bounds.memberManifestBytesPerInstaller) {
    throw new Error("package member manifest exceeds its byte bound");
  }
  return { bytes, shards };
}

export async function inspectPackageMembers({
  participant,
  installer,
  extractedRoot,
  outputDirectory,
  fixtureExecutable,
  msiMetadataPath,
  frozen,
}) {
  if (!["candidate", "control"].includes(participant)) {
    throw new Error("participant is invalid");
  }
  if (!["msi", "nsis"].includes(installer))
    throw new Error("installer is invalid");
  if (!path.isAbsolute(extractedRoot) || !path.isAbsolute(outputDirectory)) {
    throw new Error("package member inspection requires absolute directories");
  }
  const participantProfile = frozen.package.participants[participant];
  const fixture =
    participant === frozen.package.fixtureExclusion.participant
      ? await hashBoundedFile(
          fixtureExecutable,
          frozen.package.observerBounds.executableArtifactBytes,
        )
      : undefined;
  const projection = {
    ...frozen.package.inspection.installedPayloadProjection[installer],
    executableLogicalRelativePath:
      participantProfile.installedExecutableLogicalPaths[installer],
  };
  const extractedRecords = await inspectBoundedTreeMembers(
    extractedRoot,
    frozen.package.observerBounds,
  );
  const semantic =
    installer === "msi"
      ? await mapMsiInstalledMembers({
          extractedRecords,
          extractedRoot,
          metadataPath: msiMetadataPath,
          metadataMaximumBytes:
            frozen.package.observerBounds.msiMetadataBytes ?? 256 * 1024,
          maximumPathBytes: frozen.package.observerBounds.pathUtf8Bytes,
          projection,
        })
      : mapNsisInstalledMembers(extractedRecords, projection);
  const records = finalizeRecords(semantic.records);
  if (records.length < 1)
    throw new Error("package member manifest cannot be empty");
  const { bytes, shards } = await writeShards(
    records,
    outputDirectory,
    participant,
    installer,
    frozen.package.observerBounds,
  );
  const regularFiles = records.filter(
    (record) => record.type === "regularFile",
  );
  const extractedRegularFiles = extractedRecords.filter(
    (record) => record.type === "regularFile",
  );
  const basenames = frozen.package.fixtureExclusion.canonicalBasenames.map(
    (name) => name.toLocaleLowerCase("en-US"),
  );
  const webviewBasenames =
    frozen.package.webviewProvisioning.bundledPayloadBasenames.map((name) =>
      name.toLocaleLowerCase("en-US"),
    );
  const installedExecutablePath = projection.executableLogicalRelativePath;
  const sageFiles = regularFiles.filter(
    (record) =>
      record.relativePath.toLocaleLowerCase("en-US") ===
      installedExecutablePath.toLocaleLowerCase("en-US"),
  );
  if (sageFiles.length !== 1)
    throw new Error("package installed executable mapping is not exact");
  const webviewFiles = extractedRegularFiles.filter((record) =>
    webviewBasenames.includes(
      path.posix.basename(record.relativePath).toLocaleLowerCase("en-US"),
    ),
  );
  return {
    memberManifest: {
      extractedRecordCount: extractedRecords.length,
      extractedRegularFileCount: extractedRegularFiles.length,
      extractedRegularFileBytes: extractedRegularFiles.reduce(
        (total, record) => total + record.bytes,
        0,
      ),
      recordCount: records.length,
      regularFileCount: regularFiles.length,
      regularFileBytes: regularFiles.reduce(
        (total, record) => total + record.bytes,
        0,
      ),
      pathUtf8Bytes: records.reduce(
        (total, record) => total + Buffer.byteLength(record.relativePath),
        0,
      ),
      shardCount: shards.length,
      bytes,
      sha256: recordDigest(records),
      shards,
    },
    executableMapping: semantic.mapping,
    installedExecutable: {
      logicalRelativePath: sageFiles[0].relativePath,
      extractedRelativePath: semantic.installedExecutableSourceRelativePath,
      bytes: sageFiles[0].bytes,
      sha256: sageFiles[0].sha256,
    },
    fixtureBasenameCount: extractedRegularFiles.filter((record) =>
      basenames.includes(
        path.posix.basename(record.relativePath).toLocaleLowerCase("en-US"),
      ),
    ).length,
    fixtureHashMatchCount: fixture
      ? extractedRegularFiles.filter(
          (record) => record.sha256 === fixture.sha256,
        ).length
      : null,
    bundledWebviewPayloadCount: webviewFiles.length,
    bundledWebviewPayloadBytes: webviewFiles.reduce(
      (total, record) => total + record.bytes,
      0,
    ),
  };
}

function argument(name) {
  const index = process.argv.indexOf(name);
  return index < 0 ? undefined : process.argv[index + 1];
}

if (process.argv[1] && path.resolve(process.argv[1]) === scriptPath) {
  try {
    const frozen = loadFrozenContracts(defaultWorkspaceRoot);
    const value = await inspectPackageMembers({
      participant: argument("--participant"),
      installer: argument("--installer"),
      extractedRoot: argument("--extracted-root"),
      outputDirectory: argument("--output-directory"),
      fixtureExecutable: argument("--fixture-executable"),
      msiMetadataPath: argument("--msi-metadata"),
      frozen,
    });
    process.stdout.write(JSON.stringify(value));
  } catch (error) {
    process.stderr.write(`package-members: ${error.message}\n`);
    process.exitCode = 1;
  }
}
