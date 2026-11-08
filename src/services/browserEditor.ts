import type {
  BufferSnapshotAck,
  BufferSnapshotRequest,
  EditorDocument,
  EditorSaveResult,
  FileGitBase,
  ResourceRef,
  TextFile,
} from "../domain/workbench";
import {
  createInjectedEditorWatchDelivery,
  type EditorExternalWatchNotice,
  type EditorWatchSubscription,
} from "../domain/editorWatch";
import { parentResource, resourceKey } from "../domain/resourceIdentity";
import { browserDirectories } from "./browserWorkspaceFixture";

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

function open(resource: ResourceRef): EditorDocument {
  const key = resourceKey(resource);
  const currentDiskContent = diskContent(resource);
  const disk = version(currentDiskContent);
  const persisted = buffers.get(key);
  const content = persisted?.request.content ?? currentDiskContent;
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
    conflict:
      persisted && persisted.request.base.fingerprint !== disk.fingerprint
        ? {
            kind: "external-change",
            message:
              "The browser fixture changed after this buffer was opened.",
          }
        : null,
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
    fingerprint(diskContent(request.resource)) === request.base.fingerprint;
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

function save(resource: ResourceRef): EditorSaveResult {
  const key = resourceKey(resource);
  const persisted = buffers.get(key);
  if (!persisted) {
    return { outcome: "unchanged", document: open(resource) };
  }
  if (
    fingerprint(diskContent(resource)) !== persisted.request.base.fingerprint
  ) {
    return { outcome: "conflict", document: open(resource) };
  }
  files.set(key, persisted.request.content);
  buffers.delete(key);
  return { outcome: "saved", document: open(resource) };
}

function discard(resource: ResourceRef): EditorDocument {
  buffers.delete(resourceKey(resource));
  return open(resource);
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
    if (parent) {
      parent.entries = parent.entries.filter(
        (entry) => entry.resource.relativePath !== resource.relativePath,
      );
    }
  } else {
    files.set(key, content);
    deletedFiles.delete(key);
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
  createEmpty,
  resetCreatedFiles,
  watch,
  changeOnDisk,
};
import { utf8ByteLength, utf8Bytes } from "../domain/text";
