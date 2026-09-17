import { X } from "lucide-react";

import {
  attachedContextKey,
  type DiffLineCommentContext,
} from "../../domain/attachedContext";
import { attachmentLabel } from "./attachmentLabel";

export function ComposerAttachments({
  contexts,
  disabled,
  onRemove,
}: {
  contexts: readonly DiffLineCommentContext[];
  disabled: boolean;
  onRemove: (context: DiffLineCommentContext) => void;
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
            <span title={context.note}>{label}</span>
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
