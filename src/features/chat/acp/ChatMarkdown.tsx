import { Check, Copy } from "lucide-react";
import { useState } from "react";

import { renderMarkdownPreview } from "../../files/markdownPreview";
import { MermaidFigure } from "./MermaidFigure";

const languageNames: Readonly<Record<string, string>> = {
  css: "CSS",
  html: "HTML",
  js: "JavaScript",
  javascript: "JavaScript",
  json: "JSON",
  md: "Markdown",
  markdown: "Markdown",
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
}: {
  language: string;
  source: string;
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
      </header>
      <pre>
        <code>{source}</code>
      </pre>
    </section>
  );
}

export function ChatMarkdown({ text }: { text: string }) {
  const document = renderMarkdownPreview(text);
  return (
    <div className="acp-markdown">
      {document.blocks.map((block, index) => {
        if (
          block.kind === "code" &&
          block.language.toLowerCase() === "mermaid"
        ) {
          return <MermaidFigure key={`mermaid:${index}`} source={block.text} />;
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
