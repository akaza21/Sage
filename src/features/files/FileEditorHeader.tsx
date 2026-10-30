import { Eye, FilePenLine, ChevronRight } from "../../ui/leanIcons";
import { usesMetaModifier } from "../../ui/shortcutLabel";
import { unsavedState } from "./fileSurfaceCommands";
import { FileKindIcon } from "./FileKindIcon";

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

const MAX_BREADCRUMB_PATH_LENGTH = 4096;

const MAX_BREADCRUMB_SEGMENTS = 6;

function fileBreadcrumbSegments(path: string): string[] {
  if (path.length === 0 || path.length > MAX_BREADCRUMB_PATH_LENGTH) {
    return [];
  }
  const segments = path.split(/[\\/]/).filter((segment) => segment.length > 0);
  if (segments.length <= MAX_BREADCRUMB_SEGMENTS) {
    return segments;
  }
  return [segments[0]!, "…", ...segments.slice(-(MAX_BREADCRUMB_SEGMENTS - 2))];
}

function FilePathChips({
  path,
  projectName,
}: {
  path: string;
  projectName?: string;
}) {
  const pathSegments = fileBreadcrumbSegments(path);
  if (pathSegments.length === 0) {
    return <span className="file-breadcrumb__empty">{path}</span>;
  }
  const segments = projectName
    ? pathSegments.length < MAX_BREADCRUMB_SEGMENTS
      ? [projectName, ...pathSegments]
      : [
          projectName,
          pathSegments[0]!,
          "…",
          ...pathSegments.slice(-(MAX_BREADCRUMB_SEGMENTS - 3)),
        ]
    : pathSegments;
  return (
    <nav className="file-breadcrumb" aria-label="File path">
      {segments.map((segment, index) => {
        const last = index === segments.length - 1;
        return (
          <span className="file-breadcrumb__item" key={`${index}:${segment}`}>
            {index > 0 ? <ChevronRight size={12} aria-hidden="true" /> : null}
            {last ? <FileKindIcon path={path} size={14} /> : null}
            <span>{segment}</span>
          </span>
        );
      })}
    </nav>
  );
}
