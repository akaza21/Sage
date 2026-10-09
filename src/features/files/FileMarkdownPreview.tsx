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
        <MarkdownBlocks document={document} />
      ) : (
        <MermaidFigureBoundary source={source} />
      )}
      {document?.sourceLimitExceeded || document?.truncated ? (
        <p>Preview truncated</p>
      ) : null}
    </article>
  );
}
