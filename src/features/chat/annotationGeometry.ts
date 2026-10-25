/** Geometry shared by the annotation badge and its comment field (Codex s2/f0095). */

type Box = { left: number; top: number; right: number; bottom: number };

const BADGE = 24;
const FIELD_WIDTH = 270;
/** Breathing room between the field, the badge and the scrollport edges. */
const GAP = 4;
const INSET = 8;

/** A pending annotation's 24 px badge sits over the end of its last line. */
export function annotationBadgeOrigin(last: { right: number; top: number }) {
  return { left: last.right - 12, top: last.top - 26 };
}

/**
 * The comment field: min(270 px, column) wide, centred on the selection start
 * 6 px above it and clamped inside the scrollport. When it would cover the
 * badge it moves left of it, or below the line when there is no room there,
 * so the badge stays visible and the field never sits under it.
 */
export function annotationFieldBox(
  selection: { first: Box; last: Box },
  columnWidth: number,
  scrollport: Box,
  height = 44,
) {
  const width = Math.max(
    0,
    Math.min(
      FIELD_WIDTH,
      columnWidth,
      scrollport.right - scrollport.left - 2 * INSET,
    ),
  );
  const minLeft = scrollport.left + INSET;
  const maxLeft = scrollport.right - INSET - width;
  let left = Math.max(
    minLeft,
    Math.min(maxLeft, selection.first.left - width / 2),
  );
  const below = selection.last.bottom + 6;
  let top = selection.first.top - height - 6;
  if (top < scrollport.top) top = below;
  const badge = annotationBadgeOrigin(selection.last);
  const covers =
    left < badge.left + BADGE + GAP &&
    left + width > badge.left - GAP &&
    top < badge.top + BADGE &&
    top + height > badge.top;
  if (covers) {
    const beside = badge.left - GAP - width;
    if (beside >= minLeft) left = beside;
    else top = below;
  }
  return {
    left,
    top: Math.max(scrollport.top, Math.min(scrollport.bottom - height, top)),
    width,
  };
}
