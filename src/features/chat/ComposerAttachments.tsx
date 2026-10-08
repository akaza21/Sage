import { FileText, Globe2, Image, MessageSquare, X } from "lucide-react";

import {
  attachedContextKey,
  type AttachedContext,
  parseDiffLineCommentContext,
  queuedAttachmentCue,
} from "../../domain/attachedContext";
import {
  ImageAttachmentThumbnail,
  type ImageThumbnailOwner,
} from "./ImageAttachmentThumbnail";

export function QueuedAttachmentPreview({
  contexts,
  owner,
}: {
  contexts: readonly AttachedContext[];
  owner?: ImageThumbnailOwner;
}) {
  const context = contexts[0];
  if (!context)
    return (
      <span className="composer__queued-thumbnail" aria-hidden="true">
        <MessageSquare size={16} />
      </span>
    );
  const cue = queuedAttachmentCue(context);
  return (
    <span
      className="composer__queued-attachment"
      role="img"
      aria-label={`Queued attachment preview: ${cue.label} (${contexts.length} ${contexts.length === 1 ? "attachment" : "attachments"})`}
    >
      {context.kind === "image" && owner ? (
        <ImageAttachmentThumbnail
          owner={owner}
          artifactId={context.artifactId}
          label={context.label}
        />
      ) : (
        <span className="composer__queued-thumbnail" aria-hidden="true">
          {cue.kind === "page" ? (
            <Globe2 size={16} />
          ) : cue.kind === "image" ? (
            <Image size={16} />
          ) : (
            <FileText size={16} />
          )}
        </span>
      )}
      <span className="composer__queued-cue">{cue.label}</span>
    </span>
  );
}

export function attachmentLabel(context: AttachedContext): string {
  if (context.kind === "browserPage")
    return `Page · ${context.title || new URL(context.url).host}`;
  if (context.kind === "image") return `Image · ${context.label}`;
  const path = context.diffRef.relativePath;
  const file = parseDiffLineCommentContext(context) ? path : "Unavailable file";
  const range =
    context.startLine === context.endLine
      ? context.startLine
      : `${context.startLine}-${context.endLine}`;
  return `Comment · ${file}:${range}`;
}

export function ComposerAttachments({
  contexts,
  disabled,
  onRemove,
  owner,
}: {
  contexts: readonly AttachedContext[];
  disabled: boolean;
  onRemove: (context: AttachedContext) => void;
  owner?: ImageThumbnailOwner;
}) {
  if (contexts.length === 0) return null;
  return (
    <ul className="composer__attachments" aria-label="Attached context">
      {contexts.map((context) => {
        const label = attachmentLabel(context);
        return (
          <li
            className="composer__attachment"
            key={attachedContextKey(context)}
          >
            {context.kind === "image" && owner ? (
              <ImageAttachmentThumbnail
                owner={owner}
                artifactId={context.artifactId}
                label={context.label}
              />
            ) : null}
            <span
              title={
                context.kind === "browserPage"
                  ? context.url
                  : context.kind === "image"
                    ? `${context.width} × ${context.height}`
                    : context.note
              }
            >
              {label}
            </span>
            <button
              className="composer__attachment-remove"
              type="button"
              aria-label={`Remove ${label} context`}
              disabled={disabled}
              onClick={() => onRemove(context)}
            >
              <X size={12} aria-hidden="true" />
            </button>
          </li>
        );
      })}
    </ul>
  );
}
