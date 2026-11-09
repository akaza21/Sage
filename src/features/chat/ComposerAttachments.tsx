import { X } from "../../ui/leanIcons";

import {
  attachedContextKey,
  type AttachedContext,
  isBrowserElement,
  parseDiffLineCommentContext,
} from "../../domain/attachedContext";
import { BrowserElementChipsSlot } from "../browser/BrowserElementChipsSlot";

export function attachmentLabel(context: AttachedContext): string {
  if (context.kind === "transcriptQuote")
    return `Annotation ${context.ordinal}`;
  if (context.kind === "browserPage")
    return `Page · ${context.title || new URL(context.url).host}`;
  if (context.kind === "image") return `Image · ${context.label}`;
  if (context.kind === "browserElement") return `Element ${context.ordinal}`;
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
  imagePreviews = {},
}: {
  contexts: readonly AttachedContext[];
  disabled: boolean;
  onRemove: (context: AttachedContext) => void;
  imagePreviews?: Readonly<Record<string, string>>;
}) {
  if (contexts.length === 0) return null;
  const elements = contexts.filter(isBrowserElement);
  const others = contexts.filter((context) => !isBrowserElement(context));
  return (
    <ul className="composer__attachments" aria-label="Attached context">
      {elements.length ? (
        <BrowserElementChipsSlot
          {...{ contexts: elements, disabled, onRemove }}
        />
      ) : null}
      {others.map((context) => {
        const label = attachmentLabel(context);
        return (
          <li
            className="composer__attachment"
            key={attachedContextKey(context)}
          >
            {context.kind === "image" && imagePreviews[context.artifactId] ? (
              <img
                className="composer__image-preview"
                src={imagePreviews[context.artifactId]}
                alt=""
              />
            ) : null}
            <span
              title={
                context.kind === "browserPage"
                  ? context.url
                  : context.kind === "image"
                    ? `${context.width} × ${context.height}`
                    : context.kind === "transcriptQuote"
                      ? context.comment
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
