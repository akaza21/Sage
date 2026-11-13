import {
  useLayoutEffect,
  useRef,
  type KeyboardEvent,
  type RefObject,
} from "react";

import { isEditableTextTarget } from "../ui/editableTextTarget";

export function useAgentArtifactPaneFocus({
  expanded,
  overlay,
  overlayOnExpand = overlay,
  modal = false,
  paneRef,
  triggerRef,
  onExpandedChange,
}: {
  expanded: boolean;
  overlay: boolean;
  overlayOnExpand?: boolean;
  modal?: boolean;
  paneRef: RefObject<HTMLElement | null>;
  triggerRef: RefObject<HTMLButtonElement | null>;
  onExpandedChange?: (expanded: boolean) => void;
}) {
  const previous = useRef<{
    expanded: boolean;
    overlay: boolean;
    modal: boolean;
  } | null>(null);
  const preserveOutsideFocus = useRef(false);
  const restoreWhenInline = useRef(false);
  useLayoutEffect(() => {
    const before = previous.current;
    const focusInside = paneRef.current?.contains(document.activeElement);
    const resizingOutside =
      before?.expanded &&
      expanded &&
      ((!before.overlay && overlay) || (!before.modal && modal)) &&
      !focusInside;
    if (resizingOutside) {
      preserveOutsideFocus.current = true;
      restoreWhenInline.current = true;
      onExpandedChange?.(false);
    } else if (!expanded && !overlayOnExpand && restoreWhenInline.current) {
      restoreWhenInline.current = false;
      onExpandedChange?.(true);
    } else if (
      expanded &&
      overlay &&
      (!before || !before.expanded || (!before.overlay && focusInside))
    ) {
      restoreWhenInline.current = false;
      paneRef.current?.focus();
    }
    if (!expanded && before?.expanded) {
      if (!preserveOutsideFocus.current) triggerRef.current?.focus();
      preserveOutsideFocus.current = false;
    }
    previous.current = { expanded, overlay, modal };
  }, [
    expanded,
    overlay,
    overlayOnExpand,
    modal,
    paneRef,
    triggerRef,
    onExpandedChange,
  ]);

  return (event: KeyboardEvent<HTMLElement>) => {
    const pane = paneRef.current;
    if (!expanded || !pane || event.defaultPrevented) return;
    if (event.key === "Escape") {
      if (isEditableTextTarget(event.target)) return;
      event.preventDefault();
      event.stopPropagation();
      onExpandedChange?.(false);
    } else if (overlay && event.key === "Tab") {
      const controls = Array.from(
        pane.querySelectorAll<HTMLElement>(
          "button, input, select, textarea, a[href], iframe, [contenteditable=true], [tabindex]",
        ),
      ).filter((control) => {
        const style = getComputedStyle(control);
        return (
          control.tabIndex >= 0 &&
          !control.matches(":disabled, [data-artifact-focus-guard]") &&
          !control.closest("[hidden], [inert]") &&
          style.display !== "none" &&
          style.visibility !== "hidden"
        );
      });
      const boundary = event.shiftKey ? controls[0] : controls.at(-1);
      if (
        !boundary ||
        document.activeElement === boundary ||
        document.activeElement === pane
      ) {
        event.preventDefault();
        event.stopPropagation();
        (event.shiftKey ? controls.at(-1) : controls[0])?.focus();
      }
    }
  };
}
