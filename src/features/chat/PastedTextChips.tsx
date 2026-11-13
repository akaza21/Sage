import { useId, useState } from "react";

import { FileText, X } from "../../ui/leanIcons";
import { stagedItemLabel } from "./composerPasteText";
import type { StagedPromptItem } from "./pastedTextTypes";
import "../../styles/composer-paste.css";

/** Staged pasted text and files, with a preview of pasted text. */
export function PastedTextChips({
  items,
  disabled,
  onRemove,
}: {
  items: readonly StagedPromptItem[];
  disabled: boolean;
  onRemove: (id: string, label: string) => void;
}) {
  const [previewId, setPreviewId] = useState<string | null>(null);
  const previewDomId = useId();
  const preview = items.find((item) => item.id === previewId && item.text);
  return (
    <div className="composer__paste">
      <ul
        className="composer__attachments"
        aria-label="Attachments for the next message"
      >
        {items.map((item) => {
          const label = stagedItemLabel(item);
          const open = preview?.id === item.id;
          return (
            <li className="composer__attachment" key={item.id}>
              <FileText size={12} aria-hidden="true" />
              {item.text === null ? (
                <span>{label}</span>
              ) : (
                <button
                  className="composer__paste-toggle"
                  type="button"
                  aria-expanded={open}
                  aria-controls={open ? previewDomId : undefined}
                  title={open ? "Hide preview" : "Preview"}
                  onClick={() => setPreviewId(open ? null : item.id)}
                >
                  {label}
                </button>
              )}
              <button
                className="composer__attachment-remove"
                type="button"
                aria-label={`Remove ${label}`}
                disabled={disabled}
                onClick={(event) => {
                  if (open) setPreviewId(null);
                  // The chip is going away; keep focus in the composer.
                  event.currentTarget
                    .closest("form")
                    ?.querySelector("textarea")
                    ?.focus();
                  onRemove(item.id, label);
                }}
              >
                <X size={12} aria-hidden="true" />
              </button>
            </li>
          );
        })}
      </ul>
      {preview ? (
        <pre
          className="composer__paste-preview"
          id={previewDomId}
          role="region"
          aria-label={`Preview of ${stagedItemLabel(preview)}`}
          tabIndex={0}
        >
          {preview.text}
        </pre>
      ) : null}
    </div>
  );
}
