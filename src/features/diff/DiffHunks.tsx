import type { ReactNode } from "react";

import type { DiffLine, DiffLineKind, FileDiff } from "../../domain/workbench";

const linePrefix: Record<DiffLineKind, string> = {
  context: " ",
  added: "+",
  removed: "-",
  meta: "\\",
};

export function DiffHunks({
  diff,
  label,
  maxLines = Number.POSITIVE_INFINITY,
  limitMessage,
  renderComment,
}: {
  diff: FileDiff;
  label: string;
  maxLines?: number;
  limitMessage?: string;
  renderComment?: (line: DiffLine) => ReactNode;
}) {
  let remaining = maxLines;
  let clipped = diff.truncated;
  const visibleHunks = [];
  for (const hunk of diff.hunks) {
    const lines = hunk.lines.slice(0, Math.max(0, remaining));
    if (lines.length < hunk.lines.length) clipped = true;
    remaining -= lines.length;
    visibleHunks.push({ ...hunk, lines });
  }
  return (
    <div className="diff-editor" role="table" aria-label={label}>
      {diff.binary ? (
        <p className="resource-state">Binary file changed.</p>
      ) : null}
      {visibleHunks.map((hunk, hunkIndex) => {
        return hunk.lines.length > 0 ? (
          <div className="diff-hunk" key={`${hunk.header}:${hunkIndex}`}>
            <div className="diff-hunk__header" role="row">
              <span role="cell">{hunk.header}</span>
            </div>
            {hunk.lines.map((line, lineIndex) => (
              <div
                className={`diff-line diff-line--${line.kind}${renderComment ? " diff-line--commentable" : ""}`}
                role="row"
                key={`${hunkIndex}:${lineIndex}`}
              >
                <span role="cell">{line.oldLine}</span>
                <span role="cell">{line.newLine}</span>
                <div className="diff-line__comment-gutter" role="cell">
                  {renderComment?.(line)}
                </div>
                <code role="cell">
                  {linePrefix[line.kind]}
                  {line.text}
                </code>
              </div>
            ))}
          </div>
        ) : null;
      })}
      {diff.hunks.length === 0 && !diff.binary && !diff.truncated ? (
        <p className="resource-state">No textual changes for this file.</p>
      ) : null}
      {clipped && limitMessage ? (
        <p className="diff-limit">{limitMessage}</p>
      ) : null}
    </div>
  );
}
