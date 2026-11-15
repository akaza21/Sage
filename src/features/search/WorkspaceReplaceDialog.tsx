import { useId, useRef } from "react";

import { ModalDialog } from "../../ui/ModalDialog";

import type { ReplacePlan } from "./workspaceReplace";

function plural(count: number, word: string) {
  return `${count} ${word}${count === 1 ? "" : word.endsWith("h") ? "es" : "s"}`;
}

function replaceSummary(plan: ReplacePlan) {
  return `Replace ${plural(plan.matchCount, "match")} in ${plural(plan.files.length, "file")}?`;
}

export function WorkspaceReplaceDialog({
  plan,
  applying,
  rootName,
  onConfirm,
  onCancel,
}: {
  plan: ReplacePlan;
  applying: boolean;
  rootName: (rootId: string) => string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const id = useId();
  const cancelRef = useRef<HTMLButtonElement>(null);
  const empty = plan.files.length === 0;
  return (
    <ModalDialog
      className="workspace-replace-dialog"
      labelledBy={`${id}-title`}
      describedBy={`${id}-description`}
      initialFocusRef={cancelRef}
      restoreFocus
      onDismiss={onCancel}
    >
      <h2 id={`${id}-title`}>
        {empty ? "Nothing to replace" : replaceSummary(plan)}
      </h2>
      <p id={`${id}-description`}>
        {empty
          ? "No file can be changed as Search showed it."
          : `“${plan.spec.query}” becomes “${plan.spec.replacement}”. Files with unsaved edits are changed in the editor, where Undo takes the change back.`}
      </p>
      <div
        className="workspace-replace-dialog__changes"
        role="list"
        aria-label="Changes to make"
      >
        {plan.files.map((file) => (
          <section
            key={`${file.resource.rootId}\0${file.resource.relativePath}`}
            role="listitem"
            aria-label={`${file.resource.relativePath}: ${plural(file.count, "match")}`}
          >
            <h3>
              <span>
                {rootName(file.resource.rootId)} › {file.resource.relativePath}
              </span>
              <span>
                {plural(file.count, "match")}
                {file.target.kind === "disk" ? "" : " · unsaved edits"}
              </span>
            </h3>
            <ol>
              {file.lines.map((line) => (
                <li key={line.lineNumber}>
                  <span className="workspace-replace-dialog__line">
                    {line.lineNumber}
                  </span>
                  <del>{line.before || "(empty line)"}</del>
                  <ins>{line.after || "(empty line)"}</ins>
                </li>
              ))}
            </ol>
            {file.linesTruncated ? (
              <p className="workspace-replace-dialog__more">
                More changes in this file are not shown.
              </p>
            ) : null}
          </section>
        ))}
      </div>
      {plan.truncated ? (
        <p className="workspace-replace-dialog__notice">
          Only part of the results could be read; search again to replace in the
          rest.
        </p>
      ) : null}
      {plan.problems.length ? (
        <ul
          className="workspace-replace-dialog__notice"
          aria-label="Files left as they are"
        >
          {plan.problems.map((problem) => (
            <li
              key={`${problem.resource.rootId}\0${problem.resource.relativePath}`}
            >
              {problem.resource.relativePath}: {problem.message}
            </li>
          ))}
        </ul>
      ) : null}
      <div className="dirty-dialog__actions">
        <button
          ref={cancelRef}
          className="button"
          type="button"
          onClick={onCancel}
        >
          {empty ? "Close" : "Cancel"}
        </button>
        {empty ? null : (
          <button
            className="button button--primary"
            type="button"
            disabled={applying}
            onClick={onConfirm}
          >
            {applying ? "Replacing…" : "Replace"}
          </button>
        )}
      </div>
    </ModalDialog>
  );
}
