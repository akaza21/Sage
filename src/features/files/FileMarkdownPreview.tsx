import {
  renderMarkdownPreview,
  type MarkdownPreviewBlock,
} from "./markdownPreview";

export function FileMarkdownPreview({ source }: { source: string }) {
  const document = renderMarkdownPreview(source);
  return (
    <article className="file-markdown-preview" aria-label="Markdown preview">
      {document.blocks.map((block, index) => (
        <PreviewBlock key={`${block.kind}:${index}`} block={block} />
      ))}
      {document.sourceLimitExceeded || document.truncated ? (
        <p>Preview truncated</p>
      ) : null}
    </article>
  );
}

function PreviewBlock({ block }: { block: MarkdownPreviewBlock }) {
  switch (block.kind) {
    case "heading":
      if (block.level === 1) return <h1>{block.text}</h1>;
      if (block.level === 2) return <h2>{block.text}</h2>;
      return <h3>{block.text}</h3>;
    case "paragraph":
      return <p>{block.text}</p>;
    case "quote":
      return <blockquote>{block.text}</blockquote>;
    case "code":
      return <pre>{block.text}</pre>;
    case "list":
      return (
        <ul>
          {block.items.map((item, index) => (
            <li key={`${item}:${index}`}>{item}</li>
          ))}
        </ul>
      );
  }
}
