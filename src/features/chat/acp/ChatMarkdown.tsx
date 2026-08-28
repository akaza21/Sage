import { renderMarkdownPreview } from "../../files/markdownPreview";
import { MermaidFigure } from "./MermaidFigure";

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
          return <pre key={index}>{block.text}</pre>;
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
