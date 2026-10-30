import { useEffect, useMemo, useState, type CSSProperties } from "react";

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
import type { StreamRuns } from "./streamTail";
import type nextStreamRuns from "./streamTail";

/** Load streamTail lazily with this module, outside first-Chat; text streams undecorated until ready (s5/f0309). */
let tracker: typeof nextStreamRuns | null = null;
let loading: Promise<void> | null = null;

export const streamRunTracker = () => tracker;

export function loadStreamRuns(): Promise<void> {
  loading ??= import("./streamTail").then(
    (module) => {
      tracker = module.default;
    },
    () => {
      // A failed import is retried by the next streamed answer.
      loading = null;
    },
  );
  return loading;
}

void loadStreamRuns();

/** Fade only blocks appended after mount; settle existing blocks on remount to prevent flashing. */
function settleMountedStream(element: HTMLDivElement | null) {
  for (const animation of element?.getAnimations?.({ subtree: true }) ?? [])
    if ((animation as CSSAnimation).animationName === "acp-stream-in")
      animation.finish();
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
  const document = useMemo(() => renderMarkdownPreview(text), [text]);
  // One record per answer/chunk; retain appended spans so older fades finish while new words trail.
  const [stream, setStream] = useState<{
    text: string;
    document: typeof document;
    runs?: StreamRuns | null;
  }>(() => ({ text, document }));
  const trackRuns = streaming ? streamRunTracker() : null;
  if (stream.text !== text)
    setStream({
      text,
      document,
      runs: trackRuns?.(stream.runs, stream.document, document),
    });
  const streamRuns = streaming && stream.text === text ? stream.runs : null;
  useEffect(() => {
    if (streaming) void loadStreamRuns();
  }, [streaming]);
  const listMarkerWidth = useMemo(
    () =>
      document.blocks.reduce(
        (digits, block) =>
          block.kind === "list" && block.ordered
            ? Math.max(
                digits,
                String((block.start ?? 1) + block.items.length - 1).length,
              )
            : digits,
        1,
      ),
    [document],
  );
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
    <div
      ref={settleMountedStream}
      className="acp-markdown"
      style={
        { "--chat-list-marker-width": `${listMarkerWidth}ch` } as CSSProperties
      }
    >
      <MarkdownBlocks
        document={document}
        streamRuns={streamRuns}
        renderLink={renderLink}
        renderCode={(block) =>
          block.language.toLowerCase() === "mermaid" &&
          !(streaming && !block.fenceClosed) ? (
            <MermaidFigureBoundary source={block.text} copySource={false} />
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
