import { AlertTriangle } from "lucide-react";

import type { EditorConflict } from "../../domain/workbench";

interface FileEditorNoticesProps {
  conflict: EditorConflict | null;
  message: string | null;
}

export function FileEditorNotices({
  conflict,
  message,
}: FileEditorNoticesProps) {
  return (
    <>
      {conflict ? (
        <div className="editor-notice editor-notice--conflict" role="alert">
          <AlertTriangle size={16} aria-hidden="true" />
          <span>
            <strong>External change detected.</strong> {conflict.message}
          </span>
        </div>
      ) : null}
      {message && !conflict ? (
        <div className="editor-notice editor-notice--error" role="alert">
          <AlertTriangle size={16} aria-hidden="true" />
          <span>{message}</span>
        </div>
      ) : null}
    </>
  );
}
