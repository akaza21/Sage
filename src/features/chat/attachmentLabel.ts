import {
  parseDiffLineCommentContext,
  type DiffLineCommentContext,
} from "../../domain/attachedContext";

export function attachmentLabel(context: DiffLineCommentContext): string {
  const path = context.diffRef.relativePath;
  const file = parseDiffLineCommentContext(context) ? path : "Unavailable file";
  const range =
    context.startLine === context.endLine
      ? context.startLine
      : `${context.startLine}-${context.endLine}`;
  return `Comment · ${file}:${range}`;
}
