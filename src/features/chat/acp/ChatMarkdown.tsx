import { Check, Copy } from "lucide-react";
import { useMemo, useState } from "react";

import { renderMarkdownPreview } from "../../files/markdownPreview";
import { MermaidFigureBoundary } from "./MermaidFigureBoundary";

const languageNames: Readonly<Record<string, string>> = {
  css: "CSS",
  html: "HTML",
  js: "JavaScript",
  javascript: "JavaScript",
  json: "JSON",
  md: "Markdown",
  markdown: "Markdown",
  mermaid: "Mermaid",
  py: "Python",
  python: "Python",
  rs: "Rust",
  rust: "Rust",
  ts: "TypeScript",
  tsx: "TypeScript JSX",
  typescript: "TypeScript",
};

function codeLanguageLabel(language: string) {
  const normalized = language.trim().toLowerCase();
  return languageNames[normalized] ?? (normalized || "Code");
}

function ChatCodeBlock({
  language,
  source,
  streaming = false,
}: {
  language: string;
  source: string;
  streaming?: boolean;
}) {
  const [copyState, setCopyState] = useState<"idle" | "copied" | "error">(
    "idle",
  );
  const label = codeLanguageLabel(language);

  async function copySource() {
    try {
      await navigator.clipboard.writeText(source);
      setCopyState("copied");
    } catch {
      setCopyState("error");
    }
  }

  return (
    <section className="acp-code-block" aria-label={`${label} code`}>
      <header className="acp-code-block__header">
        <span>{label}</span>
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
          <button type="button" aria-label="Copy code" onClick={copySource}>
            {copyState === "copied" ? (
              <Check size={14} aria-hidden="true" />
            ) : (
              <Copy size={14} aria-hidden="true" />
            )}
            {copyState === "copied"
              ? "Copied"
              : copyState === "error"
                ? "Copy failed"
                : "Copy"}
          </button>
        </span>
      </header>
      <pre>
        <code>{source}</code>
      </pre>
    </section>
  );
}

export function ChatMarkdown({
  text,
  streaming = false,
}: {
  text: string;
  streaming?: boolean;
}) {
  // The exact text is the parser authority; unrelated projection updates do
  // not parse a retained message again.
  const document = useMemo(() => renderMarkdownPreview(text), [text]);
  return (
    <div className="acp-markdown">
      {document.blocks.map((block, index) => {
        if (
          block.kind === "code" &&
          block.language.toLowerCase() === "mermaid"
        ) {
          return streaming && !block.fenceClosed ? (
            <ChatCodeBlock
              key={`mermaid:${index}`}
              language={block.language}
              source={block.text}
              streaming
            />
          ) : (
            <MermaidFigureBoundary
              key={`mermaid:${index}`}
              source={block.text}
            />
          );
        }
        if (block.kind === "heading") {
          if (block.level === 1) return <h3 key={index}>{block.text}</h3>;
          if (block.level === 2) return <h4 key={index}>{block.text}</h4>;
          return <h5 key={index}>{block.text}</h5>;
        }
        if (block.kind === "paragraph") {
          return <p key={index}>{block.text}</p>;
        }
        if (block.kind === "quote") {
          return <blockquote key={index}>{block.text}</blockquote>;
        }
        if (block.kind === "code") {
          return (
            <ChatCodeBlock
              key={index}
              language={block.language}
              source={block.text}
            />
          );
        }
        return (
          <ul key={index}>
            {block.items.map((item, itemIndex) => (
              <li key={`${item}:${itemIndex}`}>{item}</li>
            ))}
          </ul>
        );
      })}
      {document.sourceLimitExceeded || document.truncated ? (
        <p>Chat preview truncated</p>
      ) : null}
    </div>
  );
}
