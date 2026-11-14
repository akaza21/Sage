/**
 * Where the rendered composer toolbar fit (useComposerToolbarFit) leaves the
 * context meter, measured the way the composer-compaction width sweep checks
 * it: the meter may be hidden only while a higher-priority control label is
 * compacted, or while giving its column back would overflow the measured
 * controls row. Serialised into the capture probe, so it must stay
 * self-contained.
 *
 * Returns "unknown" with no known context window (no meter slot), "pending"
 * while the slot is laid out but its meter has not mounted, "shown",
 * "dropped" when the fit justifies hiding it, or "hiddenThoughItFits".
 */
export function contextMeterFit(composer) {
  const slot = composer?.querySelector(".composer__context-slot");
  if (!slot) return "unknown";
  if (slot.getBoundingClientRect().width > 0)
    return slot.querySelector('[role="progressbar"]') ? "shown" : "pending";
  const outranking = [
    ".composer__capacity",
    ".project-scope .searchable-picker__trigger-label",
    ".composer__effort-picker[data-overflow-control] .composer__effort-label",
    ".composer__access-text",
    ".composer__harness .searchable-picker__trigger-label",
    ".composer__model-label",
  ];
  if (
    outranking.some((selector) => {
      const label = composer.querySelector(selector);
      return label !== null && label.getBoundingClientRect().width === 0;
    })
  )
    return "dropped";
  const row = composer.querySelector(".composer__controls");
  if (!row) return "unknown";
  const previous = slot.style.display;
  slot.style.display = "block";
  const edge = row.getBoundingClientRect().right + 0.5;
  const overflows = [...row.children].some(
    (child) => child.getBoundingClientRect().right > edge,
  );
  slot.style.display = previous;
  return overflows ? "dropped" : "hiddenThoughItFits";
}
