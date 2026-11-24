/**
 * Self-contained page predicates: each is serialized into the in-page
 * observation, so none may close over module scope. The DOM observation
 * module re-exports all of these for its importers.
 */
/** Kinds whose matching node must also carry this accessible name. */
// Wave 5 names the plan session mode "Plan mode" (ComposerAddActions.tsx).
// The harness picker's listbox is aria-label="Harness" (SearchablePicker);
// the model listbox is aria-labelledby its "Select model" title (ChatModelPicker).
export const NAMED_KINDS = Object.freeze({
  planEntry: "Plan mode",
  harnessMenu: "Harness",
  modelMenu: "Select model",
});
/**
 * Accessible name in accname precedence: aria-labelledby, then aria-label,
 * then text content. Self-contained: serialized into page expressions.
 */
export function accessibleName(element) {
  const normalize = (text) => text.replace(/\s+/gu, " ").trim();
  const labelledBy = (element.getAttribute("aria-labelledby") ?? "").trim();
  if (labelledBy) {
    const ids = labelledBy.split(/\s+/u);
    if (ids.length > 4) throw new Error("capture-name-bound");
    const labels = ids
      .map((id) => element.ownerDocument.getElementById(id))
      .filter(Boolean);
    if (labels.length > 0)
      return normalize(labels.map((label) => label.textContent).join(" "));
  }
  return normalize(
    element.getAttribute("aria-label") ?? element.textContent ?? "",
  );
}
export function visibleRectangle(rectangle, viewport) {
  return (
    [
      rectangle.x,
      rectangle.y,
      rectangle.width,
      rectangle.height,
      viewport.width,
      viewport.height,
    ].every(Number.isFinite) &&
    rectangle.width > 0 &&
    rectangle.height > 0 &&
    rectangle.x < viewport.width &&
    rectangle.y < viewport.height &&
    rectangle.x + rectangle.width > 0 &&
    rectangle.y + rectangle.height > 0
  );
}
/**
 * A closed <details> hides its descendants except its own summary. The element
 * itself may be a closed <details>: its summary row is still rendered.
 */
export function hiddenByClosedDetails(element) {
  for (
    let details = element.parentElement?.closest("details:not([open])");
    details;
    details = details.parentElement?.closest("details:not([open])")
  )
    if (!details.querySelector(":scope > summary")?.contains(element))
      return true;
  return false;
}
export function assistantTextRendered(turnId, document, visible) {
  return (
    Array.from(document.querySelectorAll(".acp-turn"))
      .filter((element) => element.dataset.turnId === turnId)
      .some((element) =>
        Array.from(
          element.querySelectorAll(
            ".acp-message--assistant .acp-markdown, .acp-permission:not(.acp-permission--settled) > p:first-of-type",
          ),
        ).some((text) => visible(text) && text.textContent.trim().length > 0),
      ) ||
    Array.from(
      document.querySelectorAll(
        ".acp-permission-prompt .acp-permission-prompt__question",
      ),
    ).some(
      (text) => visible(text) && (text.textContent ?? "").trim().length > 0,
    )
  );
}
export function hasVisibleLazyFallback(document, visible) {
  return Array.from(
    document.querySelectorAll(
      '.acp-chat-projection [role="status"], .chat-surface [role="status"]',
    ),
  ).some(
    (element) =>
      visible(element) &&
      /^Loading\s+.+(?:\u2026|\.\.\.)$/u.test(element.textContent.trim()),
  );
}
