import type { ResourceRef } from "./resource";

export interface TextFile {
  resource: ResourceRef;
  content: string;
  byteLen: number;
  lineEnding: "lf" | "crlf" | "none";
  modifiedAt: number | null;
}

export interface DocumentVersion {
  fingerprint: string;
  byteLen: number;
  modifiedAt: number | null;
}

export interface EditorBufferVersion {
  fingerprint: string;
  byteLen: number;
  revision: number;
  content: string;
}

export interface EditorConflict {
  /**
   * The native side reports only "external-change"; "deleted" comes from the
   * WebView's watch, which sees the File go away while it is open.
   */
  kind: "external-change" | "deleted";
  message: string;
}

export interface EditorDocument {
  resource: ResourceRef;
  disk: DocumentVersion;
  base: DocumentVersion;
  buffer: EditorBufferVersion;
  lineEnding: "lf" | "crlf" | "none";
  dirty: boolean;
  conflict: EditorConflict | null;
}

export interface BufferSnapshotRequest {
  resource: ResourceRef;
  base: DocumentVersion;
  lineEnding: "lf" | "crlf" | "none";
  content: string;
  revision: number;
}

export interface BufferSnapshotAck {
  revision: number;
  persistedAt: number;
  dirty: boolean;
}

export interface EditorSaveResult {
  outcome: "saved" | "unchanged" | "conflict";
  document: EditorDocument;
}

export interface RecoveredEditorBuffer {
  resource: ResourceRef;
  revision: number;
}

export type DiffLineKind = "context" | "added" | "removed" | "meta";

export interface DiffLine {
  kind: DiffLineKind;
  oldLine: number | null;
  newLine: number | null;
  text: string;
}

export interface DiffHunk {
  header: string;
  lines: DiffLine[];
}

/**
 * The staged (index) text of a File, which the gutter marks changes against.
 * `unchanged` answers a request that named the current blob.
 */
export type FileGitBase =
  | { state: "unavailable" }
  | { state: "unchanged"; oid: string }
  | { state: "ready"; oid: string; content: string };

export interface FileDiff {
  resource: ResourceRef;
  hunks: DiffHunk[];
  binary: boolean;
  truncated: boolean;
}
