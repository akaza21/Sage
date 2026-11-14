// Loads on the first Replace, so the Search panel carries none of it.

import { useWorkbenchStore } from "../../app/store";
import { resourceKey, sameResource } from "../../domain/resourceIdentity";
import type { EditorDocument, ResourceRef } from "../../domain/workbench";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { contentText } from "../files/editorContent";
import type {
  MatchPosition,
  PlannedFile,
  ReplaceFileProblem,
  ReplacePlan,
  ReplaceProblemKind,
  ReplaceSpec,
  WorkspaceReplaceGateway,
} from "./workspaceReplace";

export function unsavedEdits(file: ResourceRef): "editor" | "buffer" | null {
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

export function problemFrom(
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

// What Rust's `str::trim` removes, Unicode White_Space, which Search's
// preview is trimmed by; `String.prototype.trim` differs (U+0085, U+FEFF).
const RUST_WHITESPACE =
  "\\t\\n\\v\\f\\r \\u0085\\u00a0\\u1680\\u2000-\\u200a\\u2028\\u2029\\u202f\\u205f\\u3000";
const RUST_TRIM = new RegExp(
  `^[${RUST_WHITESPACE}]+|[${RUST_WHITESPACE}]+$`,
  "gu",
);

/** Search trims a line's preview and cuts a long one short with an ellipsis. */
export function sameLine(line: string, preview: string): boolean {
  const trimmed = line.replace(RUST_TRIM, "");
  return preview.endsWith("\u2026")
    ? trimmed.startsWith(preview.slice(0, -1))
    : trimmed === preview;
}

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
      const before = file.lines[0]?.before;
      if (only && onlyLine !== undefined && before !== onlyLine) {
        problems.push({
          resource: file.resource,
          kind: "stale",
          message: "it changed since the search; search again",
        });
        continue;
      }
      files.push({
        ...file,
        target: { kind: "disk", fingerprint: file.fingerprint },
      });
    }
  }
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
