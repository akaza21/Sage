import { createHash } from "node:crypto";
import { lstat, open } from "node:fs/promises";
import path from "node:path";

import { recordDigest } from "./ade0006a-c0b-common.mjs";

function sameIdentity(left, right) {
  return (
    left.dev === right.dev &&
    left.ino === right.ino &&
    left.size === right.size &&
    left.mtimeNs === right.mtimeNs
  );
}

async function readMetadata(filePath, maximumBytes) {
  const before = await lstat(filePath, { bigint: true });
  if (
    !before.isFile() ||
    before.isSymbolicLink() ||
    before.size < 1n ||
    before.size > BigInt(maximumBytes)
  ) {
    throw new Error("MSI metadata must be one bounded regular file");
  }
  const handle = await open(filePath, "r");
  try {
    const opened = await handle.stat({ bigint: true });
    if (!sameIdentity(before, opened))
      throw new Error("MSI metadata identity changed while opening");
    const bytes = Buffer.alloc(Number(opened.size));
    let offset = 0;
    while (offset < bytes.byteLength) {
      const result = await handle.read(
        bytes,
        offset,
        bytes.byteLength - offset,
        offset,
      );
      if (result.bytesRead === 0)
        throw new Error("MSI metadata ended before its declared size");
      offset += result.bytesRead;
    }
    const [after, pathAfter] = await Promise.all([
      handle.stat({ bigint: true }),
      lstat(filePath, { bigint: true }),
    ]);
    if (
      pathAfter.isSymbolicLink() ||
      !sameIdentity(opened, after) ||
      !sameIdentity(opened, pathAfter)
    ) {
      throw new Error("MSI metadata identity changed while reading");
    }
    return {
      bytes,
      sha256: createHash("sha256").update(bytes).digest("hex"),
    };
  } finally {
    await handle.close();
  }
}

function decodeXmlAttribute(value) {
  return value.replace(
    /&(?:amp|quot|apos|lt|gt|#\d+|#x[0-9a-fA-F]+);/gu,
    (entity) => {
      if (entity === "&amp;") return "&";
      if (entity === "&quot;") return '"';
      if (entity === "&apos;") return "'";
      if (entity === "&lt;") return "<";
      if (entity === "&gt;") return ">";
      const numeric = entity.startsWith("&#x")
        ? Number.parseInt(entity.slice(3, -1), 16)
        : Number.parseInt(entity.slice(2, -1), 10);
      if (!Number.isSafeInteger(numeric) || numeric < 1 || numeric > 0x10ffff)
        throw new Error("MSI metadata contains an invalid XML entity");
      return String.fromCodePoint(numeric);
    },
  );
}

function attributes(source) {
  const result = {};
  const pattern = /\s+([A-Za-z_][A-Za-z0-9_.:-]*)="([^"<]*)"/gy;
  let offset = 0;
  while (offset < source.length) {
    pattern.lastIndex = offset;
    const match = pattern.exec(source);
    if (!match) {
      if (/^\s*$/u.test(source.slice(offset))) break;
      throw new Error("MSI metadata contains malformed attributes");
    }
    if (Object.hasOwn(result, match[1]))
      throw new Error("MSI metadata contains a duplicate attribute");
    result[match[1]] = decodeXmlAttribute(match[2]);
    offset = pattern.lastIndex;
  }
  return result;
}

function safeSegment(value, label) {
  if (
    typeof value !== "string" ||
    value.length < 1 ||
    value === "." ||
    value === ".." ||
    /[\\/\0:]/u.test(value)
  ) {
    throw new Error(`MSI ${label} is not a safe path segment`);
  }
  return value;
}

function within(child, parent) {
  const relative = path.relative(parent, child);
  return (
    relative === "" ||
    (!relative.startsWith("..") && !path.isAbsolute(relative))
  );
}

function parseMappings(xml, extractedRoot, maximumPathBytes, projection) {
  if (/<!DOCTYPE|<!ENTITY|<!\[CDATA\[/iu.test(xml))
    throw new Error("MSI metadata contains forbidden XML constructs");
  const directoryStack = [];
  const mappings = [];
  let installRootCount = 0;
  const tags = /<(\/)?([A-Za-z_][A-Za-z0-9_.:-]*)([^<>]*?)(\/?)>/gu;
  for (const match of xml.matchAll(tags)) {
    const closing = Boolean(match[1]);
    const name = match[2].includes(":")
      ? match[2].slice(match[2].lastIndexOf(":") + 1)
      : match[2];
    if (name === "Directory") {
      if (closing) {
        if (directoryStack.length === 0)
          throw new Error("MSI metadata has an unmatched Directory close");
        directoryStack.pop();
        continue;
      }
      const values = attributes(match[3]);
      const parent = directoryStack.at(-1);
      let logicalSegments = parent?.logicalSegments;
      if (values.Id === projection.logicalRoot) {
        installRootCount += 1;
        logicalSegments = [projection.logicalRoot];
      } else if (logicalSegments) {
        logicalSegments = [
          ...logicalSegments,
          safeSegment(values.Name, "directory name"),
        ];
      }
      directoryStack.push({ logicalSegments });
      if (match[4]) directoryStack.pop();
      continue;
    }
    if (name !== "File" || closing) continue;
    const current = directoryStack.at(-1);
    if (!current?.logicalSegments)
      throw new Error("MSI contains an installed file outside INSTALLDIR");
    const values = attributes(match[3]);
    const fileName = safeSegment(values.Name, "file name");
    if (!path.isAbsolute(values.Source ?? ""))
      throw new Error("MSI file source is not absolute");
    const source = path.resolve(values.Source);
    if (!within(source, extractedRoot))
      throw new Error("MSI file source escapes the extraction root");
    const sourceRelativePath = path
      .relative(extractedRoot, source)
      .split(path.sep)
      .join("/");
    const relativePath = [...current.logicalSegments, fileName].join("/");
    if (
      !sourceRelativePath.startsWith(`${projection.payloadStreamRoot}/`) ||
      Buffer.byteLength(relativePath) > maximumPathBytes
    ) {
      throw new Error("MSI installed-file mapping is outside its contract");
    }
    mappings.push({ relativePath, sourceRelativePath });
  }
  if (
    directoryStack.length !== 0 ||
    installRootCount !== 1 ||
    mappings.length < 1
  )
    throw new Error("MSI metadata has an incomplete INSTALLDIR mapping");
  for (const key of ["relativePath", "sourceRelativePath"]) {
    const values = new Set(
      mappings.map((mapping) => mapping[key].toLocaleLowerCase("en-US")),
    );
    if (values.size !== mappings.length)
      throw new Error(`MSI metadata contains a duplicate ${key}`);
  }
  return mappings;
}

function mappedRecords(extractedRecords, mappings, projection) {
  const regularFiles = new Map(
    extractedRecords
      .filter((record) => record.type === "regularFile")
      .map((record) => [
        record.relativePath.toLocaleLowerCase("en-US"),
        record,
      ]),
  );
  const allowedRoots = new Set(
    [projection.payloadStreamRoot, ...projection.extractorMetadataRoots].map(
      (value) => value.toLocaleLowerCase("en-US"),
    ),
  );
  for (const record of extractedRecords) {
    const root = record.relativePath
      .split("/", 1)[0]
      .toLocaleLowerCase("en-US");
    if (!allowedRoots.has(root))
      throw new Error("MSI extraction contains an unknown metadata root");
  }
  const payloadRoot = `${projection.payloadStreamRoot.toLocaleLowerCase("en-US")}/`;
  const fileStreams = [...regularFiles.keys()].filter((value) =>
    value.startsWith(payloadRoot),
  );
  if (fileStreams.length !== mappings.length)
    throw new Error("MSI File streams do not reconcile with metadata");
  const records = [];
  const directories = new Set();
  for (const mapping of mappings) {
    const source = regularFiles.get(
      mapping.sourceRelativePath.toLocaleLowerCase("en-US"),
    );
    if (!source)
      throw new Error("MSI metadata references a missing File stream");
    const segments = mapping.relativePath.split("/");
    for (let index = 1; index < segments.length; index += 1) {
      directories.add(segments.slice(0, index).join("/"));
    }
    records.push({
      relativePath: mapping.relativePath,
      type: "regularFile",
      bytes: source.bytes,
      sha256: source.sha256,
    });
  }
  for (const relativePath of directories) {
    records.push({ relativePath, type: "directory" });
  }
  return records;
}

export async function mapMsiInstalledMembers({
  extractedRecords,
  extractedRoot,
  metadataPath,
  metadataMaximumBytes,
  maximumPathBytes,
  projection,
}) {
  const metadata = await readMetadata(metadataPath, metadataMaximumBytes);
  let xml = metadata.bytes.toString("utf8");
  if (xml.includes("\ufffd"))
    throw new Error("MSI metadata is not valid UTF-8");
  if (xml.charCodeAt(0) === 0xfeff) xml = xml.slice(1);
  const mappings = parseMappings(
    xml,
    extractedRoot,
    maximumPathBytes,
    projection,
  );
  const executableMappings = mappings.filter(
    (mapping) =>
      mapping.relativePath.toLocaleLowerCase("en-US") ===
      projection.executableLogicalRelativePath.toLocaleLowerCase("en-US"),
  );
  if (executableMappings.length !== 1)
    throw new Error("MSI installed executable mapping is not exact");
  return {
    records: mappedRecords(extractedRecords, mappings, projection),
    installedExecutableSourceRelativePath:
      executableMappings[0].sourceRelativePath,
    mapping: {
      method: projection.method,
      metadataSha256: metadata.sha256,
      mappedFileCount: mappings.length,
      sha256: recordDigest(mappings),
    },
  };
}
