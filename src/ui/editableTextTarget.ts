const TEXT_FIELD = "input, textarea, select, [contenteditable=true]";

/**
 * A read-only editor stays focusable (and contenteditable) so the keyboard can
 * reach it, but it edits nothing, so shared keys such as Escape go to its
 * container.
 */
export function isEditableTextTarget(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  const field = target.closest(TEXT_FIELD);
  return (
    field !== null &&
    field.getAttribute("aria-readonly") !== "true" &&
    !(field instanceof HTMLInputElement && field.readOnly) &&
    !(field instanceof HTMLTextAreaElement && field.readOnly)
  );
}
