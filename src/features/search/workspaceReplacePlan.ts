// Workspace Replace: previews what a Replace changes, then applies it. Files
// on disk are changed natively, each at once and only if unchanged since the
// preview; a File with unsaved edits is changed in its editor buffer instead,
// so the user's edits stay theirs and undo still works.
// This chunk loads on the first Replace, so the Search panel carries only the
// Replace field and these Files' shapes.

import { useWorkbenchStore } from "../../app/store";
import { isRecord } from "../../domain/guards";
import { resourceKey, sameResource } from "../../domain/resourceIdentity";
import type { EditorDocument, ResourceRef } from "../../domain/workbench";
import { invokeNative } from "../../ports/nativeInvoke";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { contentText } from "../files/editorContent";
import type {
  MatchPosition,
  PlannedFile,
  ReplaceFileProblem,
  ReplaceLinePreview,
  ReplaceOutcome,
  ReplacePlan,
  ReplaceProblemKind,
  ReplaceSpec,
  TextReplaceResult,
  WorkspaceReplaceGateway,
} from "./workspaceReplace";

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
 * Whether `line` is the line Search previewed as `preview`. Search trims a
 * line's preview and cuts a long one short with an ellipsis, so a long
 * line is compared only as far as its preview goes.
 */
function sameLine(line: string, preview: string): boolean {
  const trimmed = line.trim();
  return preview.endsWith("\u2026")
    ? trimmed.startsWith(preview.slice(0, -1))
    : trimmed === preview;
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
  onlyLine,
}: {
  gateway: WorkspaceReplaceGateway;
  projectId: string;
  transitionEpoch: number;
  spec: ReplaceSpec;
  resources: ResourceRef[];
  only?: MatchPosition;
  /** The text Search showed for the line of `only`. */
  onlyLine?: string;
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
      if (only && onlyLine !== undefined) {
        const line = source.split("\n")[only.lineNumber - 1];
        if (line === undefined || !sameLine(line, onlyLine)) {
          throw new Error("it was edited since the search; search again");
        }
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
async function replaceInEditor(
  file: ResourceRef,
  text: string,
  content: string,
) {
  const { replaceEditorText } = await import("../files/editorTextReplace");
  const result = replaceEditorText(resourceKey(file), text, content);
  if (result === "missing") throw new Error("its editor is not showing");
  if (result === "changed") {
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
  const revision = document.buffer.revision + 1;
  const ack = await editor.snapshotEditorBuffer({
    resource: file,
    base: document.base,
    lineEnding: document.lineEnding,
    content,
    revision,
  });
  // An older revision back means another write got there first and the
  // buffer kept it.
  if (ack.revision !== revision) {
    throw new Error("it was edited since the preview; search again");
  }
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
        await replaceInEditor(file.resource, target.text, target.content);
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
    let applied: Awaited<ReturnType<WorkspaceReplaceGateway["apply"]>>;
    try {
      applied = await gateway.apply({
        projectId: plan.projectId,
        expectedTransitionEpoch: plan.transitionEpoch,
        spec: plan.spec,
        files: onDisk.map((file) => ({
          resource: file.resource,
          fingerprint:
            file.target.kind === "disk" ? file.target.fingerprint : "",
          ...(plan.only ? { only: plan.only } : {}),
        })),
      });
    } catch (error) {
      // The editors' changes are made and stay counted; the Files on disk
      // are reported, since none of them is known to be written.
      for (const file of onDisk) {
        problems.push(problemFrom(file.resource, "refused", error));
      }
      return { matchCount, fileCount, problems };
    }
    problems.push(...applied.problems);
    for (const file of applied.replaced) {
      matchCount += file.count;
      fileCount += 1;
      try {
        await reloadIfClean(file.resource);
      } catch {
        // The File is replaced; a failed reload leaves the editor as it is.
      }
    }
  }
  return { matchCount, fileCount, problems };
}

/**
 * A clean File showing in the editor is read again after a disk Replace.
 * The editor's revision is taken now and checked again when the queued
 * reload runs, so an edit made in between is never dropped.
 */
async function reloadIfClean(file: ResourceRef) {
  const state = useWorkbenchStore.getState();
  const editor = state.activeEditor;
  if (editor?.phase !== "ready" || !sameResource(editor.resource, file)) {
    return;
  }
  if (unsavedEdits(file)) return;
  await state.discardActiveEditor({
    resource: file,
    revision: editor.revision,
  });
}
