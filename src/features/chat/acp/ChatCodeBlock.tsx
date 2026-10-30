import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Copy, Check } from "../../../ui/leanIcons";
import {
  editorLanguageForPath,
  editorLanguageStatusLabel,
} from "../../files/editorLanguage";
import type { CodeSpan } from "../../files/CodeMirrorEditorRuntime";

const LANGUAGE_ALIASES: Readonly<Record<string, string>> = {
  javascript: "js",
  typescript: "ts",
  python: "py",
  rust: "rs",
};

export function ChatCodeBlock({
  language,
  source,
  streaming = false,
  wrapInitially = false,
}: {
  language: string;
  source: string;
  streaming?: boolean;
  wrapInitially?: boolean;
}) {
  const normalized = language.trim().toLowerCase();
  const path = `f.${LANGUAGE_ALIASES[normalized] ?? normalized}`;
  const admitted = editorLanguageForPath(path);
  const label =
    (editorLanguageStatusLabel(path) ??
      (normalized === "mermaid" ? "Mermaid" : normalized)) +
    (admitted.endsWith("-jsx") ? " JSX" : "");
  const identity = useMemo(() => ({ source, language }), [source, language]);
  const [copyState, setCopyState] = useState<{
    identity: typeof identity;
    status: string;
  } | null>(null);
  const status = copyState?.identity === identity ? copyState.status : "";
  const CopyIcon = status === "Copied" ? Check : Copy;
  const [wrap, setWrap] = useState(wrapInitially);
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
  if (
    highlight &&
    highlight.identity.language === language &&
    source.startsWith(highlight.identity.source)
  )
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
  return (
    <section
      ref={region}
      className="acp-code-block"
      aria-label={label ? `${label} code` : "Code block"}
    >
      <header className="acp-code-block__header">
        {label ? (
          <span className="acp-code-block__language">
            <span aria-hidden="true">&lt;/&gt;</span> {label}
          </span>
        ) : null}
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
            <span className="acp-code-block__wrap-icon" aria-hidden="true" />
          </button>
          <span role="status">{status}</span>
          <button
            type="button"
            aria-label="Copy code"
            title="Copy code"
            data-copy-status={status || undefined}
            onClick={async () => {
              setCopyState({ identity, status: "" });
              let status = "Copied";
              try {
                await navigator.clipboard.writeText(source);
              } catch {
                status = "Copy failed";
              }
              setCopyState((current) =>
                current?.identity === identity ? { identity, status } : current,
              );
            }}
          >
            <CopyIcon size={14} aria-hidden="true" />
          </button>
        </span>
      </header>
      <pre
        tabIndex={0}
        style={{
          whiteSpace: wrap ? "pre-wrap" : "pre",
        }}
      >
        <code>{parts}</code>
      </pre>
    </section>
  );
}
