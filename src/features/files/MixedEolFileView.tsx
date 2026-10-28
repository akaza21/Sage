import { useEffect, useState } from "react";
import type { ResourceRef } from "../../domain/workbench";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { ResourceStateNotice } from "../../ui/ResourceStateNotice";
import { MAX_EDITABLE_CONTENT_BYTES } from "./editorContent";
import "./mixed-eol-file-view.css";

export function MixedEolFileView({ resource }: { resource: ResourceRef }) {
  const [result, setResult] = useState<{
    content: string;
    clipped: boolean;
  } | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    let cancelled = false;
    void getWorkbenchPorts()
      .editor.readTextFile(resource)
      .then((file) => {
        if (!cancelled)
          setResult({
            content: file.content.slice(0, MAX_EDITABLE_CONTENT_BYTES),
            clipped: file.content.length > MAX_EDITABLE_CONTENT_BYTES,
          });
      })
      .catch((cause: unknown) => {
        if (!cancelled) setError(String(cause).slice(0, 512));
      });
    return () => {
      cancelled = true;
    };
  }, [resource]);
  return (
    <section
      className="file-surface resource-document"
      aria-label={`File viewer ${resource.relativePath}`}
    >
      <p role="status">
        Mixed line endings · View only. The file is not normalized or saved.
      </p>
      {error ? (
        <ResourceStateNotice
          state="error"
          title="Could not read file"
          detail={error}
        />
      ) : result ? (
        <>
          <textarea
            className="mixed-eol-file-view"
            aria-label={`View ${resource.relativePath}`}
            value={result.content}
            readOnly
            spellCheck={false}
          />
          {result.clipped ? <p>View capped at 1 MiB of text.</p> : null}
        </>
      ) : (
        <ResourceStateNotice state="loading" title="Loading file…" />
      )}
    </section>
  );
}
