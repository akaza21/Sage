import { MermaidFigureBoundary } from "../chat/acp/MermaidFigureBoundary";
import { renderMarkdownPreview } from "./markdownPreview";
import { MarkdownBlocks } from "./MarkdownBlocks";

export function FileMarkdownPreview({
  source,
  asDiagram = false,
}: {
  source: string;
  asDiagram?: boolean;
}) {
  const document = asDiagram ? null : renderMarkdownPreview(source);
  return (
    <article
      className="file-markdown-preview"
      aria-label={asDiagram ? "Diagram preview" : "Markdown preview"}
    >
      {document ? (
        <MarkdownBlocks
          document={document}
          renderCode={(block) =>
            block.language.toLowerCase() === "mermaid" ? (
              <MermaidFigureBoundary source={block.text} />
            ) : (
              <pre
                tabIndex={0}
                role="region"
                aria-label={`${block.language || "Plain text"} code`}
              >
                <code>{block.text}</code>
              </pre>
            )
          }
        />
      ) : (
        <MermaidFigureBoundary source={source} />
      )}
      {document?.sourceLimitExceeded || document?.truncated ? (
        <p>Preview truncated</p>
      ) : null}
    </article>
  );
}
