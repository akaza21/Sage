import type { KeyboardEvent } from "react";

function dialogControls(container: HTMLElement) {
  return Array.from(
    container.querySelectorAll<HTMLElement>(
      "button, input, select, textarea, a[href], [tabindex]",
    ),
  ).filter((item) => {
    const style = getComputedStyle(item);
    return (
      item.tabIndex >= 0 &&
      !item.matches(":disabled") &&
      !item.closest("[hidden], [inert]") &&
      style.display !== "none" &&
      style.visibility !== "hidden"
    );
  });
}

export function focusPopoverDialog(popover: HTMLElement) {
  const focus = () => {
    if (popover.isConnected && !popover.hidden) {
      (
        popover.querySelector<HTMLElement>(
          "button:not(:disabled), input:not(:disabled)",
        ) ?? popover
      ).focus();
    }
  };
  focus();
  queueMicrotask(focus);
}

export function exitPopoverDialogOnTab(
  event: KeyboardEvent<HTMLElement>,
  trigger: HTMLElement | null,
  onDismiss: () => void,
) {
  const popover = event.currentTarget;
  const controls = dialogControls(popover);
  // A lazy dialog can gain controls after its initial container focus.
  if (document.activeElement === popover && controls.length) {
    event.preventDefault();
    (event.shiftKey ? controls.at(-1) : controls[0])?.focus();
    return;
  }
  const boundary = event.shiftKey ? controls[0] : controls.at(-1);
  if (!boundary || document.activeElement === boundary) {
    event.preventDefault();
    event.stopPropagation();
    trigger?.focus();
    onDismiss();
  }
}
