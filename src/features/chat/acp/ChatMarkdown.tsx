import { useEffect, useMemo, useState } from "react";

import { renderMarkdownPreview } from "../../files/markdownPreview";
import { MarkdownBlocks } from "../../files/MarkdownBlocks";
import type { ResourceRef } from "../../../domain/workbench";
import type { AgentArtifactOwner } from "../../../domain/agentArtifact";
import { acpDesktop } from "../../../services/acpDesktop";
import {
  agentArtifactOwnerIsCurrent,
  openAgentArtifact,
} from "../../../app/openAgentArtifact";
import { useWorkbenchStore } from "../../../app/store";
import { MermaidFigureBoundary } from "./MermaidFigureBoundary";
import { ChatCodeBlock } from "./ChatCodeBlock";

export function ChatMarkdown({
  text,
  streaming = false,
  fileReferenceOwner,
}: {
  text: string;
  streaming?: boolean;
  fileReferenceOwner?: AgentArtifactOwner;
}) {
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
    links: ({
      resource: ResourceRef;
      lineNumber: number | null;
    } | null)[];
  } | null>(null);
  useEffect(() => {
    let current = true;
    if (ownerKey && projectId) {
      void Promise.all(
        document.fileLinks.map(async (link) => {
          try {
            return await acpDesktop.resolveChatFileReference(
              projectId,
              link.destination,
            );
          } catch {
            return null;
          }
        }),
      ).then((links) => {
        if (current)
          setResolvedLinks({
            document,
            ownerKey,
            links,
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
  const renderLink = (index: number) => {
    const link = fileLinks[index];
    if (!link) return document.fileLinks[index].label;
    return (
      <button
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
        {document.fileLinks[index].label}
      </button>
    );
  };
  return (
    <div className="acp-markdown">
      <MarkdownBlocks
        document={document}
        renderLink={renderLink}
        renderCode={(block) =>
          block.language.toLowerCase() === "mermaid" &&
          !(streaming && !block.fenceClosed) ? (
            <MermaidFigureBoundary source={block.text} />
          ) : (
            <ChatCodeBlock
              language={block.language}
              source={block.text}
              streaming={
                streaming &&
                !block.fenceClosed &&
                block.language.toLowerCase() === "mermaid"
              }
            />
          )
        }
      />
      {document.sourceLimitExceeded || document.truncated ? (
        <p>Chat preview truncated</p>
      ) : null}
    </div>
  );
}
