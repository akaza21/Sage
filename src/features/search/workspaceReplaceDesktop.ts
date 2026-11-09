// The native side of Workspace Replace: previews, applies and replaces in
// text, each response checked before it is used.

import { isRecord } from "../../domain/guards";
import type { ResourceRef } from "../../domain/workbench";
import { invokeNative } from "../../ports/nativeInvoke";
import type {
  ReplaceFileProblem,
  ReplaceLinePreview,
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
      value.kind !== "noMatches" &&
      value.kind !== "keptUnsaved")
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
