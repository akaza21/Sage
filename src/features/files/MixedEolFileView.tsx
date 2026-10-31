import { useEffect, useRef, useState } from "react";
import {
  noticeMatchesWatch,
  type EditorWatchSubscription,
} from "../../domain/editorWatch";
import { truncateUtf8, utf8ByteLength } from "../../domain/text";
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
  const [watchError, setWatchError] = useState("");
  const refresh = useRef<() => void>(() => undefined);
  const cancellation = useRef<Promise<void>>(Promise.resolve());
  useEffect(() => {
    let cancelled = false;
    let reading = false;
    let pending = false;
    let version = 0;
    let subscription: EditorWatchSubscription | undefined;
    let unsubscribe: (() => void) | undefined;
    const editor = getWorkbenchPorts().editor;
    const requestRead = () => {
      if (cancelled) return;
      version += 1;
      pending = true;
      setError("");
      if (reading) return;
      reading = true;
      void (async () => {
        while (pending && !cancelled) {
          pending = false;
          const requestedVersion = version;
          try {
            const file = await editor.readTextFile(resource);
            if (cancelled || requestedVersion !== version) continue;
            setResult({
              content: truncateUtf8(file.content, MAX_EDITABLE_CONTENT_BYTES),
              clipped:
                utf8ByteLength(file.content) > MAX_EDITABLE_CONTENT_BYTES,
            });
          } catch (cause: unknown) {
            if (!cancelled && requestedVersion === version)
              setError(String(cause).slice(0, 512));
          }
        }
        reading = false;
      })();
    };
    refresh.current = requestRead;
    const start = cancellation.current.then(async () => {
      try {
        const watch = await editor.watchEditorDocument(resource);
        if (cancelled) {
          await watch.cancel();
          return;
        }
        subscription = watch;
        unsubscribe = watch.delivery.subscribe((notice) => {
          if (cancelled || !noticeMatchesWatch(notice, resource)) return;
          if (notice.kind === "deleted") {
            version += 1;
            pending = false;
            setResult(null);
            setError("This file was deleted outside Sage.");
          } else requestRead();
        });
        setWatchError("");
      } catch {
        if (!cancelled)
          setWatchError(
            "Automatic file watching is unavailable. Use Refresh file to check for changes.",
          );
      }
      requestRead();
    });
    return () => {
      cancelled = true;
      pending = false;
      unsubscribe?.();
      cancellation.current = start
        .then(() => subscription?.cancel())
        .catch(() => undefined);
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
      <button type="button" onClick={() => refresh.current()}>
        Refresh file
      </button>
      {watchError ? <p role="status">{watchError}</p> : null}
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
