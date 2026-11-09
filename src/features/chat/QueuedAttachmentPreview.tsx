import { FileText, Globe2, Image, MessageSquare } from "../../ui/leanIcons";

import {
  type AttachedContext,
  queuedAttachmentCue,
} from "../../domain/attachedContext";

export function QueuedAttachmentPreview({
  contexts,
}: {
  contexts: readonly AttachedContext[];
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
      <span className="composer__queued-thumbnail" aria-hidden="true">
        {cue.kind === "page" ? (
          <Globe2 size={16} />
        ) : cue.kind === "image" ? (
          <Image size={16} />
        ) : (
          <FileText size={16} />
        )}
      </span>
      <span className="composer__queued-cue">{cue.label}</span>
    </span>
  );
}
