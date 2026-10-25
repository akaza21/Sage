import { Eye, FilePenLine } from "../../ui/leanIcons";

import { usesMetaModifier } from "../../ui/shortcutLabel";
import { FilePathChips } from "./FilePathChips";
import { unsavedState } from "./saveShortcut";

interface FileEditorHeaderProps {
  path: string;
  projectName?: string;
  lineEnding: string;
  diskByteLen: number;
  persisting: boolean;
  dirty: boolean;
  saving: boolean;
  transitioning: boolean;
  canPreview?: boolean;
  previewing?: boolean;
  agentMarkdown?: boolean;
  onTogglePreview?: () => void;
}

export function FileEditorHeader({
  path,
  projectName,
  lineEnding,
  diskByteLen,
  persisting,
  dirty,
  saving,
  transitioning,
  canPreview = false,
  previewing = false,
  agentMarkdown = false,
  onTogglePreview,
}: FileEditorHeaderProps) {
  const saveState = saving
    ? { label: "Saving…", accessibleLabel: "Saving file" }
    : persisting
      ? {
          label: "Protecting changes…",
          accessibleLabel: "Protecting recovery copy",
        }
      : dirty
        ? unsavedState(usesMetaModifier())
        : null;

  return (
    <header className="file-header">
      <span className="file-header__path">
        <FilePathChips path={path} projectName={projectName} />
      </span>
      <span className="file-header__metadata">
        UTF-8 · {lineEnding.toUpperCase()} · {diskByteLen.toLocaleString()} disk
        bytes
      </span>
      {saveState || canPreview ? (
        <span className="file-header__actions">
          {saveState ? (
            <span
              className={`file-header__save-state${dirty ? " file-header__save-state--dirty" : ""}`}
              role="status"
              aria-label={saveState.accessibleLabel}
              title={saveState.accessibleLabel}
            >
              <span className="file-header__save-dot" aria-hidden="true" />
              {saveState.label}
            </span>
          ) : null}
          {canPreview ? (
            <button
              className="button button--compact file-header__preview"
              type="button"
              aria-label={
                agentMarkdown
                  ? previewing
                    ? "View source"
                    : "View preview"
                  : previewing
                    ? "Return to editor"
                    : "Preview file"
              }
              aria-pressed={previewing}
              disabled={transitioning}
              onClick={onTogglePreview}
            >
              {previewing ? (
                <FilePenLine size={14} aria-hidden="true" />
              ) : (
                <Eye size={14} aria-hidden="true" />
              )}
              {agentMarkdown
                ? previewing
                  ? "View source"
                  : "View preview"
                : previewing
                  ? "Edit"
                  : "Preview"}
            </button>
          ) : null}
        </span>
      ) : null}
    </header>
  );
}
