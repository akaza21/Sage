// Workspace Replace: previews what a Replace changes, then applies it. Files
// on disk are changed natively, each at once and only if unchanged since the
// preview; a File with unsaved edits is changed in its editor buffer instead,
// so the user's edits stay theirs and undo still works.

import { useWorkbenchStore } from "../../app/store";
import type { EditorTextReplace } from "../../domain/editorTextReplace";
import { isRecord } from "../../domain/guards";
import { resourceKey, sameResource } from "../../domain/resourceIdentity";
import type { EditorDocument, ResourceRef } from "../../domain/workbench";
import { invokeNative } from "../../ports/nativeInvoke";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { contentText } from "../files/editorContent";

/** Mirrors `EDITOR_TEXT_REPLACE_EVENT`'s listener in the File surface. */
const EDITOR_TEXT_REPLACE_EVENT = "sage:editor-text-replace";

export interface ReplaceSpec {
  query: string;
  replacement: string;
  caseSensitive: boolean;
  wholeWord: boolean;
  useRegex: boolean;
  preserveCase: boolean;
}

export interface MatchPosition {
  lineNumber: number;
  columnNumber: number;
}

export interface ReplaceLinePreview extends MatchPosition {
  before: string;
  after: string;
}

export type ReplaceProblemKind = "stale" | "refused" | "noMatches";

export interface ReplaceFileProblem {
  resource: ResourceRef;
  kind: ReplaceProblemKind;
  message: string;
}

export interface ReplaceFilePreview {
  resource: ResourceRef;
  fingerprint: string;
  count: number;
  lines: ReplaceLinePreview[];
  linesTruncated: boolean;
}

export interface TextReplaceResult {
  content: string;
  count: number;
  lines: ReplaceLinePreview[];
  linesTruncated: boolean;
}

export interface WorkspaceReplaceGateway {
  preview(request: {
    projectId: string;
    expectedTransitionEpoch: number;
    spec: ReplaceSpec;
    resources: ResourceRef[];
    only?: MatchPosition;
  }): Promise<{
    files: ReplaceFilePreview[];
    problems: ReplaceFileProblem[];
    truncated: boolean;
  }>;
  apply(request: {
    projectId: string;
    expectedTransitionEpoch: number;
    spec: ReplaceSpec;
    files: {
      resource: ResourceRef;
      fingerprint: string;
      only?: MatchPosition;
    }[];
  }): Promise<{
    replaced: { resource: ResourceRef; count: number; fingerprint: string }[];
    problems: ReplaceFileProblem[];
  }>;
  replaceText(request: {
    spec: ReplaceSpec;
    text: string;
    only?: MatchPosition;
  }): Promise<TextReplaceResult>;
}

const invalid = () => new Error("invalid workspace Replace response");

function count(value: unknown): number {
  if (!Number.isSafeInteger(value) || (value as number) < 0) throw invalid();
  return value as number;
}

function text(value: unknown): string {
  if (typeof value !== "string") throw invalid();
  return value;
}

function resource(value: unknown): ResourceRef {
  if (!isRecord(value)) throw invalid();
  return {
    rootId: text(value.rootId),
    relativePath: text(value.relativePath),
  };
}

function list<T>(value: unknown, item: (entry: unknown) => T): T[] {
  if (!Array.isArray(value)) throw invalid();
  return value.map(item);
}

function linePreview(value: unknown): ReplaceLinePreview {
  if (!isRecord(value)) throw invalid();
  return {
    lineNumber: count(value.lineNumber),
    columnNumber: count(value.columnNumber),
    before: text(value.before),
    after: text(value.after),
  };
}

function problem(value: unknown): ReplaceFileProblem {
  if (
    !isRecord(value) ||
    (value.kind !== "stale" &&
      value.kind !== "refused" &&
      value.kind !== "noMatches")
  ) {
    throw invalid();
  }
  return {
    resource: resource(value.resource),
    kind: value.kind,
    message: text(value.message),
  };
}

function textResult(value: unknown): TextReplaceResult {
  if (!isRecord(value)) throw invalid();
  return {
    content: text(value.content),
    count: count(value.count),
    lines: list(value.lines, linePreview),
    linesTruncated: value.linesTruncated === true,
  };
}

export const workspaceReplaceDesktop: WorkspaceReplaceGateway = {
  async preview(request) {
    const value = await invokeNative("preview_workspace_replace", { request });
    if (!isRecord(value)) throw invalid();
    return {
      files: list(value.files, (file) => {
        if (!isRecord(file)) throw invalid();
        return {
          resource: resource(file.resource),
          fingerprint: text(file.fingerprint),
          count: count(file.count),
          lines: list(file.lines, linePreview),
          linesTruncated: file.linesTruncated === true,
        };
      }),
      problems: list(value.problems, problem),
      truncated: value.truncated === true,
    };
  },
  async apply(request) {
    const value = await invokeNative("apply_workspace_replace", { request });
    if (!isRecord(value)) throw invalid();
    return {
      replaced: list(value.replaced, (file) => {
        if (!isRecord(file)) throw invalid();
        return {
          resource: resource(file.resource),
          count: count(file.count),
          fingerprint: text(file.fingerprint),
        };
      }),
      problems: list(value.problems, problem),
    };
  },
  async replaceText(request) {
    return textResult(
      await invokeNative("replace_workspace_text", { request }),
    );
  },
};

/** Where a planned File's change goes. */
export type ReplaceTarget =
  /** On disk, natively. */
  | { kind: "disk"; fingerprint: string }
  /** In the editor showing the File, as one undoable edit. */
  | { kind: "editor"; text: string; content: string }
  /** In the unsaved buffer of a File open in another tab. */
  | { kind: "buffer"; document: EditorDocument; content: string };

export interface PlannedFile {
  resource: ResourceRef;
  count: number;
  lines: ReplaceLinePreview[];
  linesTruncated: boolean;
  target: ReplaceTarget;
}

export interface ReplacePlan {
  projectId: string;
  transitionEpoch: number;
  spec: ReplaceSpec;
  /** Set when the Replace is of one match. */
  only?: MatchPosition;
  files: PlannedFile[];
  problems: ReplaceFileProblem[];
  truncated: boolean;
  matchCount: number;
}

export interface ReplaceOutcome {
  matchCount: number;
  fileCount: number;
  problems: ReplaceFileProblem[];
}

/** Where the File's unsaved edits live, if it has any. */
function unsavedEdits(file: ResourceRef): "editor" | "buffer" | null {
  const state = useWorkbenchStore.getState();
  const tab = state.tabs.find(
    (candidate) =>
      candidate.kind === "file" &&
      !!candidate.resource &&
      sameResource(candidate.resource, file),
  );
  if (!tab?.dirty) return null;
  const editor = state.activeEditor;
  return editor?.phase === "ready" && sameResource(editor.resource, file)
    ? "editor"
    : "buffer";
}

function problemFrom(
  file: ResourceRef,
  kind: ReplaceProblemKind,
  error: unknown,
): ReplaceFileProblem {
  return {
    resource: file,
    kind,
    message: error instanceof Error ? error.message : String(error),
  };
}

/**
 * Works out what a Replace of `resources` would change, without changing
 * anything: from disk for most Files, and from the unsaved text for a File
 * with unsaved edits.
 */
export async function planReplace({
  gateway,
  projectId,
  transitionEpoch,
  spec,
  resources,
  only,
}: {
  gateway: WorkspaceReplaceGateway;
  projectId: string;
  transitionEpoch: number;
  spec: ReplaceSpec;
  resources: ResourceRef[];
  only?: MatchPosition;
}): Promise<ReplacePlan> {
  const files: PlannedFile[] = [];
  const problems: ReplaceFileProblem[] = [];
  const onDisk: ResourceRef[] = [];
  for (const file of resources) {
    const edits = unsavedEdits(file);
    if (!edits) {
      onDisk.push(file);
      continue;
    }
    try {
      let source: string;
      let document: EditorDocument | null = null;
      if (edits === "editor") {
        const editor = useWorkbenchStore.getState().activeEditor;
        if (editor?.phase !== "ready") throw new Error("the editor closed");
        source = contentText(editor.content);
      } else {
        document = await getWorkbenchPorts().editor.openEditorDocument(file);
        source = document.buffer.content;
      }
      const result = await gateway.replaceText({ spec, text: source, only });
      if (!result.count) {
        problems.push({
          resource: file,
          kind: "noMatches",
          message: "nothing in its unsaved text matches",
        });
        continue;
      }
      files.push({
        resource: file,
        count: result.count,
        lines: result.lines,
        linesTruncated: result.linesTruncated,
        target: document
          ? { kind: "buffer", document, content: result.content }
          : { kind: "editor", text: source, content: result.content },
      });
    } catch (error) {
      problems.push(problemFrom(file, "refused", error));
    }
  }
  let truncated = false;
  if (onDisk.length) {
    const preview = await gateway.preview({
      projectId,
      expectedTransitionEpoch: transitionEpoch,
      spec,
      resources: onDisk,
      ...(only ? { only } : {}),
    });
    truncated = preview.truncated;
    problems.push(...preview.problems);
    for (const file of preview.files) {
      files.push({
        ...file,
        target: { kind: "disk", fingerprint: file.fingerprint },
      });
    }
  }
  // Results order, whichever way each File was read.
  const order = new Map(
    resources.map((file, index) => [resourceKey(file), index]),
  );
  files.sort(
    (left, right) =>
      (order.get(resourceKey(left.resource)) ?? 0) -
      (order.get(resourceKey(right.resource)) ?? 0),
  );
  return {
    projectId,
    transitionEpoch,
    spec,
    only,
    files,
    problems,
    truncated,
    matchCount: files.reduce((total, file) => total + file.count, 0),
  };
}

/** Puts `content` in the editor showing `file`, if it still shows `text`. */
function replaceInEditor(file: ResourceRef, text: string, content: string) {
  const detail: EditorTextReplace = {
    resourceKey: resourceKey(file),
    expected: text,
    content,
    handled: false,
    applied: false,
  };
  window.dispatchEvent(new CustomEvent(EDITOR_TEXT_REPLACE_EVENT, { detail }));
  if (!detail.handled) throw new Error("its editor is not showing");
  if (!detail.applied) {
    throw new Error("it was edited since the preview; search again");
  }
}

/** Writes `content` to the unsaved buffer of `file`, if it is as planned. */
async function replaceInBuffer(
  file: ResourceRef,
  planned: EditorDocument,
  content: string,
) {
  const editor = getWorkbenchPorts().editor;
  const document = await editor.openEditorDocument(file);
  if (
    document.buffer.revision !== planned.buffer.revision ||
    document.buffer.content !== planned.buffer.content
  ) {
    throw new Error("it was edited since the preview; search again");
  }
  await editor.snapshotEditorBuffer({
    resource: file,
    base: document.base,
    lineEnding: document.lineEnding,
    content,
    revision: document.buffer.revision + 1,
  });
}

/** Applies `plan`, File by File; what could not be done is reported. */
export async function applyReplacePlan(
  plan: ReplacePlan,
  gateway: WorkspaceReplaceGateway,
): Promise<ReplaceOutcome> {
  const problems: ReplaceFileProblem[] = [];
  let matchCount = 0;
  let fileCount = 0;
  const onDisk = plan.files.filter((file) => file.target.kind === "disk");
  for (const file of plan.files) {
    const { target } = file;
    if (target.kind === "disk") continue;
    try {
      if (target.kind === "editor") {
        replaceInEditor(file.resource, target.text, target.content);
      } else {
        await replaceInBuffer(file.resource, target.document, target.content);
      }
      matchCount += file.count;
      fileCount += 1;
    } catch (error) {
      problems.push(problemFrom(file.resource, "stale", error));
    }
  }
  if (onDisk.length) {
    const applied = await gateway.apply({
      projectId: plan.projectId,
      expectedTransitionEpoch: plan.transitionEpoch,
      spec: plan.spec,
      files: onDisk.map((file) => ({
        resource: file.resource,
        fingerprint: file.target.kind === "disk" ? file.target.fingerprint : "",
        ...(plan.only ? { only: plan.only } : {}),
      })),
    });
    problems.push(...applied.problems);
    for (const file of applied.replaced) {
      matchCount += file.count;
      fileCount += 1;
      await reloadIfClean(file.resource);
    }
  }
  return { matchCount, fileCount, problems };
}

/** A clean File showing in the editor is read again after a disk Replace. */
async function reloadIfClean(file: ResourceRef) {
  const state = useWorkbenchStore.getState();
  const editor = state.activeEditor;
  if (editor?.phase !== "ready" || !sameResource(editor.resource, file)) {
    return;
  }
  if (unsavedEdits(file)) return;
  await state.discardActiveEditor();
}
