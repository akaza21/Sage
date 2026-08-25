import { Eye, FileCode2, FilePenLine, Save, Undo2 } from "lucide-react";

interface FileEditorHeaderProps {
  path: string;
  lineEnding: string;
  diskByteLen: number;
  persisting: boolean;
  dirty: boolean;
  saving: boolean;
  transitioning: boolean;
  canPreview?: boolean;
  previewing?: boolean;
  onTogglePreview?: () => void;
  onDiscard: () => void;
  onSave: () => void;
}

export function FileEditorHeader({
  path,
  lineEnding,
  diskByteLen,
  persisting,
  dirty,
  saving,
  transitioning,
  canPreview = false,
  previewing = false,
  onTogglePreview,
  onDiscard,
  onSave,
}: FileEditorHeaderProps) {
  return (
    <header className="file-header">
      <span className="file-header__path">
        <FileCode2 size={16} aria-hidden="true" />
        {path}
      </span>
      <span className="file-header__metadata">
        UTF-8 · {lineEnding.toUpperCase()} · {diskByteLen.toLocaleString()} disk
        bytes
        {persisting ? " · Recovering…" : ""}
      </span>
      <span className="file-header__actions">
        {canPreview ? (
          <button
            className="button button--compact"
            type="button"
            disabled={transitioning}
            onClick={onTogglePreview}
          >
            {previewing ? (
              <FilePenLine size={14} aria-hidden="true" />
            ) : (
              <Eye size={14} aria-hidden="true" />
            )}
            {previewing ? "Edit" : "Preview"}
          </button>
        ) : null}
        <button
          className="button button--compact"
          type="button"
          disabled={!dirty || saving || transitioning}
          onClick={onDiscard}
        >
          <Undo2 size={14} aria-hidden="true" />
          Discard
        </button>
        <button
          className="button button--compact button--primary"
          type="button"
          disabled={!dirty || saving || persisting || transitioning}
          onClick={onSave}
        >
          <Save size={14} aria-hidden="true" />
          {saving ? "Saving…" : "Save"}
        </button>
      </span>
    </header>
  );
}
