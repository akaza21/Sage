type Box = { left: number; top: number; right: number; bottom: number };

const BADGE = 24;
const FIELD_WIDTH = 270;
const GAP = 4;
const INSET = 8;

export function annotationBadgeOrigin(last: Omit<Box, "left">) {
  return {
    left: last.right - BADGE / 2,
    top: last.top - BADGE,
  };
}

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
  let left = Math.max(
    minLeft,
    Math.min(
      scrollport.right - INSET - width,
      selection.first.left - width / 2,
    ),
  );
  const badge = annotationBadgeOrigin(selection.last);
  const beside = badge.left - GAP - width;
  if (beside >= minLeft) left = Math.min(left, beside);
  let top = selection.first.top - height - 6;
  if (left + width > badge.left - GAP)
    top = Math.min(top, badge.top - height - GAP);
  if (top < scrollport.top) top = selection.last.bottom + 6;
  return {
    left,
    top: Math.max(scrollport.top, Math.min(scrollport.bottom - height, top)),
    width,
  };
}
