import { Check, Copy } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { renderMarkdownPreview } from "../../files/markdownPreview";
import type { ResourceRef } from "../../../domain/workbench";
import type { AgentArtifactOwner } from "../../../domain/agentArtifact";
import { acpDesktop } from "../../../services/acpDesktop";
import {
  agentArtifactOwnerIsCurrent,
  openAgentArtifact,
} from "../../../app/openAgentArtifact";
import { useWorkbenchStore } from "../../../app/store";
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
  fileReferenceOwner,
}: {
  text: string;
  streaming?: boolean;
  fileReferenceOwner?: AgentArtifactOwner;
}) {
  // The exact text is the parser authority; unrelated projection updates do
  // not parse a retained message again.
  const document = useMemo(() => renderMarkdownPreview(text), [text]);
  const ownerKey = fileReferenceOwner
    ? JSON.stringify([
        fileReferenceOwner.projectId,
        fileReferenceOwner.chatId,
        fileReferenceOwner.sessionId,
      ])
    : null;
  const projectId = fileReferenceOwner?.projectId;
  const [resolvedLinks, setResolvedLinks] = useState<{
    document: typeof document;
    ownerKey: string;
    links: {
      label: string;
      resource: ResourceRef;
      lineNumber: number | null;
      blockIndex: number;
      itemIndex?: number;
      offset: number;
    }[];
  } | null>(null);
  useEffect(() => {
    let current = true;
    if (ownerKey && projectId) {
      void Promise.all(
        document.fileLinks.map(async (link) => {
          try {
            const resolved = await acpDesktop.resolveChatFileReference(
              projectId,
              link.destination,
            );
            return resolved ? { ...link, ...resolved } : null;
          } catch {
            return null;
          }
        }),
      ).then((links) => {
        if (current)
          setResolvedLinks({
            document,
            ownerKey,
            links: links.filter((link) => link !== null),
          });
      });
    }
    return () => {
      current = false;
    };
  }, [document, ownerKey, projectId]);
  const fileLinks =
    resolvedLinks?.document === document && resolvedLinks.ownerKey === ownerKey
      ? resolvedLinks.links
      : [];
  const renderInline = (
    value: string,
    blockIndex: number,
    itemIndex?: number,
  ) => {
    const links = fileLinks
      .filter(
        (link) =>
          link.blockIndex === blockIndex && link.itemIndex === itemIndex,
      )
      .sort((left, right) => left.offset - right.offset);
    const parts: React.ReactNode[] = [];
    let position = 0;
    for (const link of links) {
      if (
        link.offset < position ||
        value.slice(link.offset, link.offset + link.label.length) !== link.label
      )
        continue;
      parts.push(value.slice(position, link.offset));
      parts.push(
        <button
          key={`${link.offset}:${link.resource.rootId}:${link.resource.relativePath}`}
          type="button"
          className="acp-resource-action"
          aria-label={`Open ${link.resource.relativePath}`}
          onClick={() => {
            if (!fileReferenceOwner) return;
            void (async () => {
              const opened = await openAgentArtifact({
                owner: fileReferenceOwner,
                target: "file",
                resource: link.resource,
              });
              if (!opened || link.lineNumber === null) return;
              const workbench = useWorkbenchStore.getState();
              if (
                !agentArtifactOwnerIsCurrent(
                  fileReferenceOwner,
                  workbench.projectGeneration,
                )
              )
                return;
              await workbench.openFileLocation({
                projectId: fileReferenceOwner.projectId,
                transitionEpoch: workbench.nativeTransitionEpoch,
                resource: link.resource,
                lineNumber: link.lineNumber,
                columnNumber: 1,
                selection: { kind: "caret" },
              });
            })();
          }}
        >
          {link.label}
        </button>,
      );
      position = link.offset + link.label.length;
    }
    parts.push(value.slice(position));
    return parts;
  };
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
          if (block.level === 1)
            return <h3 key={index}>{renderInline(block.text, index)}</h3>;
          if (block.level === 2)
            return <h4 key={index}>{renderInline(block.text, index)}</h4>;
          return <h5 key={index}>{renderInline(block.text, index)}</h5>;
        }
        if (block.kind === "paragraph") {
          return <p key={index}>{renderInline(block.text, index)}</p>;
        }
        if (block.kind === "quote") {
          return (
            <blockquote key={index}>
              {renderInline(block.text, index)}
            </blockquote>
          );
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
              <li key={`${item}:${itemIndex}`}>
                {renderInline(item, index, itemIndex)}
              </li>
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
