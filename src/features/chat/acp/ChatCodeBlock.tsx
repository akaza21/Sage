import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Copy, Check } from "../../../ui/leanIcons";
import {
  editorLanguageForPath,
  editorLanguageStatusLabel,
} from "../../files/editorLanguage";
import type { CodeSpan } from "../../files/CodeMirrorEditorRuntime";

export function ChatCodeBlock({
  language,
  source,
  streaming = false,
}: {
  language: string;
  source: string;
  streaming?: boolean;
}) {
  const normalized = language.trim().toLowerCase();
  const path = `f.${({ javascript: "js", typescript: "ts", python: "py", rust: "rs" } as Record<string, string>)[normalized] ?? normalized}`;
  const admitted = editorLanguageForPath(path);
  const label =
    (editorLanguageStatusLabel(path) ??
      (normalized === "mermaid" ? "Mermaid" : normalized)) +
    (admitted.endsWith("-jsx") ? " JSX" : "");
  const identity = useMemo(() => ({ source, language }), [source, language]);
  const [copyState, setCopyState] = useState<{
    identity: typeof identity;
    result: string;
  } | null>(null);
  const [wrap, setWrap] = useState(false);
  const [highlight, setHighlight] = useState<{
    identity: typeof identity;
    spans: CodeSpan[];
  } | null>(null);
  const region = useRef<HTMLElement>(null);
  useEffect(() => {
    if (admitted === "plain") return;
    let current = true;
    const load = () => {
      void import("../../files/CodeMirrorEditorRuntime")
        .then((module) =>
          current
            ? module.highlightCodeSource(source, admitted, () => current)
            : null,
        )
        .then((spans) => {
          if (current && spans) setHighlight({ identity, spans });
        })
        .catch(() => {});
    };
    const observer =
      typeof IntersectionObserver === "undefined"
        ? null
        : new IntersectionObserver((entries) => {
            if (entries.some((entry) => entry.isIntersecting)) {
              observer?.disconnect();
              load();
            }
          });
    if (observer && region.current) observer.observe(region.current);
    else load();
    return () => {
      current = false;
      observer?.disconnect();
    };
  }, [admitted, identity, source]);
  const parts: ReactNode[] = [];
  let offset = 0;
  if (highlight?.identity === identity)
    for (const span of highlight.spans) {
      parts.push(
        source.slice(offset, span.from),
        <span key={span.from} className={span.className}>
          {source.slice(span.from, span.to)}
        </span>,
      );
      offset = span.to;
    }
  parts.push(source.slice(offset));
  const status = copyState?.identity === identity ? copyState.result : "";
  return (
    <section
      ref={region}
      className="acp-code-block"
      aria-label={label ? `${label} code` : "Code block"}
    >
      <header className="acp-code-block__header">
        {label ? <span>{label}</span> : null}
        <span className="acp-code-block__actions">
          {streaming ? (
            <span
              className="acp-code-block__streaming"
              role="status"
              aria-label="Receiving diagram source"
            >
              Receiving…
            </span>
          ) : null}
          <button
            type="button"
            aria-label="Wrap code"
            aria-pressed={wrap}
            title="Wrap code"
            onClick={() => setWrap(!wrap)}
          >
            <svg
              width="14"
              height="14"
              viewBox="0 0 16 16"
              fill="none"
              stroke="currentColor"
              aria-hidden="true"
            >
              <path d="M2 3h12M2 7h9a3 3 0 0 1 0 6H7m2-2-2 2 2 2M2 11h2" />
            </svg>
          </button>
          <span role="status">{status}</span>
          <button
            type="button"
            aria-label="Copy code"
            title="Copy code"
            onClick={async () => {
              setCopyState({ identity, result: "" });
              let result = "Copied";
              try {
                await navigator.clipboard.writeText(source);
              } catch {
                result = "Copy failed";
              }
              setCopyState((state) =>
                state?.identity === identity ? { identity, result } : state,
              );
            }}
          >
            {status === "Copied" ? (
              <Check size={14} aria-hidden="true" />
            ) : (
              <Copy size={14} aria-hidden="true" />
            )}
          </button>
        </span>
      </header>
      <pre
        tabIndex={0}
        style={{
          whiteSpace: wrap ? "pre-wrap" : "pre",
          overflowWrap: wrap ? "anywhere" : undefined,
        }}
      >
        <code>{parts}</code>
      </pre>
    </section>
  );
}
