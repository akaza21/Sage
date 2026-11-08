import {
  ABSENT_FINGERPRINT,
  type BufferRebaseRequest,
  type BufferSnapshotAck,
  type BufferSnapshotRequest,
  type EditorDocument,
  type EditorSaveResult,
  type FileGitBase,
  type ResourceRef,
  type TextFile,
  type WorkspaceEntry,
} from "../domain/workbench";
import {
  createInjectedEditorWatchDelivery,
  type EditorExternalWatchNotice,
  type EditorWatchSubscription,
} from "../domain/editorWatch";
import { parentResource, resourceKey } from "../domain/resourceIdentity";
import { browserDirectories } from "./browserWorkspaceFixture";

const ABSENT_VERSION = {
  fingerprint: ABSENT_FINGERPRINT,
  byteLen: 0,
  modifiedAt: null,
};

const files = new Map<string, string>();
const buffers = new Map<
  string,
  { request: BufferSnapshotRequest; fingerprint: string }
>();
const fixtureCreatedFiles = new Set<string>();
// Files deleted "outside Sage" by a test, and the open Files' watches.
const deletedFiles = new Set<string>();
const watchers = new Map<
  string,
  Set<(notice: EditorExternalWatchNotice) => void>
>();
let watchGeneration = 0;

function diskContent(resource: ResourceRef): string {
  if (deletedFiles.has(resourceKey(resource))) {
    throw new Error("The file does not exist.");
  }
  return (
    files.get(resourceKey(resource)) ??
    `// Browser projection for ${resource.relativePath}\n`
  );
}

function fingerprint(content: string): string {
  let hash = 2_166_136_261;
  for (const byte of utf8Bytes(content)) {
    hash ^= byte;
    hash = Math.imul(hash, 16_777_619);
  }
  return `browser-${(hash >>> 0).toString(16).padStart(8, "0")}`;
}

function version(content: string) {
  return {
    fingerprint: fingerprint(content),
    byteLen: utf8ByteLength(content),
    modifiedAt: null,
  };
}

/**
 * The fixture has no index: a File's staged text is its text on disk, so
 * edits in the browser renderer get gutter markers.
 */
function stagedText(
  resource: ResourceRef,
  knownOid: string | null,
): FileGitBase {
  const content = diskContent(resource);
  const oid = fingerprint(content);
  return oid === knownOid
    ? { state: "unchanged", oid }
    : { state: "ready", oid, content };
}

function readTextFile(resource: ResourceRef): TextFile {
  const content = diskContent(resource);
  return {
    resource,
    content,
    byteLen: utf8ByteLength(content),
    lineEnding: "lf",
    modifiedAt: Date.now(),
  };
}

/** The File's text, or `null` when a test deleted it. */
function diskIfPresent(resource: ResourceRef): string | null {
  return deletedFiles.has(resourceKey(resource)) ? null : diskContent(resource);
}

function diskVersion(resource: ResourceRef) {
  const content = diskIfPresent(resource);
  return content === null ? ABSENT_VERSION : version(content);
}

/** Mirrors the native open: a deleted File with a buffer opens as it. */
function open(resource: ResourceRef): EditorDocument {
  const key = resourceKey(resource);
  const persisted = buffers.get(key);
  // Without a buffer, a deleted File cannot be opened.
  const currentDiskContent = persisted
    ? diskIfPresent(resource)
    : diskContent(resource);
  const disk =
    currentDiskContent === null ? ABSENT_VERSION : version(currentDiskContent);
  const content = persisted?.request.content ?? currentDiskContent ?? "";
  const moved =
    persisted && persisted.request.base.fingerprint !== disk.fingerprint;
  return {
    resource,
    disk,
    base: persisted?.request.base ?? disk,
    buffer: {
      fingerprint: persisted?.fingerprint ?? disk.fingerprint,
      byteLen: utf8ByteLength(content),
      revision: persisted?.request.revision ?? 0,
      content,
    },
    lineEnding: persisted?.request.lineEnding ?? "lf",
    dirty: Boolean(persisted),
    conflict: !moved
      ? null
      : currentDiskContent === null
        ? DELETED
        : {
            kind: "external-change",
            message:
              "The browser fixture changed after this buffer was opened.",
          },
  };
}

const DELETED = {
  kind: "deleted",
  message: "This file was deleted outside Sage.",
} as const;

function deletedDocument(resource: ResourceRef): EditorDocument {
  return {
    resource,
    disk: ABSENT_VERSION,
    base: ABSENT_VERSION,
    buffer: { ...ABSENT_VERSION, revision: 0, content: "" },
    lineEnding: "none",
    dirty: false,
    conflict: DELETED,
  };
}

function snapshot(request: BufferSnapshotRequest): BufferSnapshotAck {
  const key = resourceKey(request.resource);
  const persisted = buffers.get(key);
  if (persisted) {
    if (persisted.request.base.fingerprint !== request.base.fingerprint) {
      throw new Error("buffer base does not match the durable base");
    }
    if (request.revision <= persisted.request.revision) {
      return {
        revision: persisted.request.revision,
        persistedAt: Date.now(),
        dirty: true,
      };
    }
  }
  const bufferFingerprint = fingerprint(request.content);
  const diskMatchesBase =
    diskVersion(request.resource).fingerprint === request.base.fingerprint;
  if (!persisted && !diskMatchesBase) {
    throw new Error(
      "snapshot base no longer matches disk and no durable buffer exists",
    );
  }
  const dirty =
    bufferFingerprint !== request.base.fingerprint || !diskMatchesBase;
  if (dirty) {
    buffers.set(key, { request, fingerprint: bufferFingerprint });
  } else {
    buffers.delete(key);
  }
  return { revision: request.revision, persistedAt: Date.now(), dirty };
}

/** A buffer kept over a deletion creates its File, which must not exist. */
function save(resource: ResourceRef): EditorSaveResult {
  const key = resourceKey(resource);
  const persisted = buffers.get(key);
  if (!persisted) {
    return { outcome: "unchanged", document: open(resource) };
  }
  if (
    diskVersion(resource).fingerprint !== persisted.request.base.fingerprint
  ) {
    return { outcome: "conflict", document: open(resource) };
  }
  if (deletedFiles.delete(key)) restoreListing(resource);
  files.set(key, persisted.request.content);
  buffers.delete(key);
  return { outcome: "saved", document: open(resource) };
}

/** Drops the buffer whether or not the File still exists, as native does. */
/** Mirrors the native discard; `keepIfAbsent` is Reload's. */
function discard(resource: ResourceRef, keepIfAbsent = false): EditorDocument {
  const key = resourceKey(resource);
  if (diskIfPresent(resource) === null) {
    if (keepIfAbsent && buffers.has(key)) return open(resource);
    if (!keepIfAbsent) buffers.delete(key);
    return deletedDocument(resource);
  }
  buffers.delete(key);
  return open(resource);
}

function rebase(request: BufferRebaseRequest): EditorSaveResult {
  const key = resourceKey(request.resource);
  const disk = diskVersion(request.resource);
  if (disk.fingerprint !== request.expectedDisk.fingerprint) {
    // Mirrors the native rebase: disk holding the editor's text matches;
    // anything else is a conflict, whatever the recovery copy's base.
    if (disk.fingerprint === fingerprint(request.content)) {
      buffers.delete(key);
      return { outcome: "matched", document: open(request.resource) };
    }
    const document = open(request.resource);
    document.conflict ??=
      disk === ABSENT_VERSION
        ? DELETED
        : {
            kind: "external-change",
            message:
              "The browser fixture changed after this buffer was opened.",
          };
    return { outcome: "conflict", document };
  }
  // Mirrors the native refusal to create a kept File in protected metadata.
  const gitName = request.resource.relativePath
    .split("/")
    .find((name) => name.toLowerCase() === ".git");
  if (disk === ABSENT_VERSION && gitName) {
    throw new Error(
      `invalid request: files cannot be created inside ${gitName}`,
    );
  }
  const bufferFingerprint = fingerprint(request.content);
  if (bufferFingerprint === disk.fingerprint) {
    buffers.delete(key);
  } else {
    buffers.set(key, {
      request: { ...request, base: disk },
      fingerprint: bufferFingerprint,
    });
  }
  return { outcome: "rebased", document: open(request.resource) };
}

function createEmpty(resource: ResourceRef) {
  const key = resourceKey(resource);
  if (files.has(key)) throw new Error("file already exists");
  files.set(key, "");
  deletedFiles.delete(key);
  fixtureCreatedFiles.add(key);
}

function resetCreatedFiles() {
  for (const key of fixtureCreatedFiles) {
    files.delete(key);
    buffers.delete(key);
  }
  fixtureCreatedFiles.clear();
  deletedFiles.clear();
  removedEntries.clear();
}

/** One watch per open File, as the native watch keeps. */
function watch(resource: ResourceRef): EditorWatchSubscription {
  const key = resourceKey(resource);
  const injected = createInjectedEditorWatchDelivery();
  const listeners = watchers.get(key) ?? new Set();
  listeners.add(injected.emit);
  watchers.set(key, listeners);
  return {
    resource,
    delivery: injected.delivery,
    cancel: async () => {
      listeners.delete(injected.emit);
      if (listeners.size === 0) watchers.delete(key);
      injected.close();
    },
  };
}

const removedEntries = new Map<string, WorkspaceEntry>();

function restoreListing(resource: ResourceRef) {
  const entry = removedEntries.get(resourceKey(resource));
  const parent = browserDirectories.get(resourceKey(parentResource(resource)));
  if (entry && parent && !parent.entries.includes(entry)) {
    parent.entries = [...parent.entries, entry];
  }
}

/**
 * Test fixture: another program writes the File (`content`) or deletes it
 * (`null`), and its watches hear about it as the native watch reports.
 */
function changeOnDisk(resource: ResourceRef, content: string | null) {
  const key = resourceKey(resource);
  if (content === null) {
    files.delete(key);
    deletedFiles.add(key);
    const parent = browserDirectories.get(
      resourceKey(parentResource(resource)),
    );
    const removed = parent?.entries.find(
      (entry) => entry.resource.relativePath === resource.relativePath,
    );
    if (parent && removed) {
      removedEntries.set(key, removed);
      parent.entries = parent.entries.filter((entry) => entry !== removed);
    }
  } else {
    files.set(key, content);
    if (deletedFiles.delete(key)) restoreListing(resource);
  }
  watchGeneration += 1;
  for (const emit of watchers.get(key) ?? []) {
    emit({
      resource,
      generation: watchGeneration,
      kind: content === null ? "deleted" : "changed",
    });
  }
}

export const browserEditor = {
  readTextFile,
  stagedText,
  open,
  snapshot,
  save,
  discard,
  rebase,
  createEmpty,
  resetCreatedFiles,
  watch,
  changeOnDisk,
};
import { utf8ByteLength, utf8Bytes } from "../domain/text";
