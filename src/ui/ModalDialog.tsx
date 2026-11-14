import {
  useEffect,
  useRef,
  type KeyboardEvent,
  type ReactNode,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";
import { DismissLayerContext, useDismissLayer } from "./dismissLayer";
import { useNativePageLayer } from "./nativePageLayers";

const FOCUSABLE_SELECTOR = [
  'a[href]:not([tabindex="-1"])',
  'button:not([disabled]):not([tabindex="-1"])',
  'input:not([disabled]):not([type="hidden"]):not([tabindex="-1"])',
  'select:not([disabled]):not([tabindex="-1"])',
  'textarea:not([disabled]):not([tabindex="-1"])',
  '[tabindex]:not([tabindex="-1"])',
].join(",");

function isVisible(element: HTMLElement) {
  if (element.closest('[hidden], [aria-hidden="true"]')) return false;
  const style = window.getComputedStyle(element);
  return style.display !== "none" && style.visibility !== "hidden";
}

function focusableElements(dialog: HTMLDialogElement) {
  return Array.from(
    dialog.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR),
  ).filter(isVisible);
}

export function ModalDialog({
  children,
  className,
  labelledBy,
  describedBy,
  initialFocusRef,
  restoreFocus = false,
  onDismiss,
}: {
  children: ReactNode;
  className?: string;
  labelledBy: string;
  describedBy?: string;
  initialFocusRef?: RefObject<HTMLElement | null>;
  /**
   * Off by default because most dialogs choose where the focus goes when they
   * close.
   */
  restoreFocus?: boolean;
  onDismiss: () => void;
}) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const previousFocusRef = useRef(document.activeElement);
  useNativePageLayer(dialogRef, { coverage: "window" });
  const { layer, onKeyDown: layerKeyDown } = useDismissLayer(true, {
    inside: [dialogRef],
    onEscape: onDismiss,
    modal: true,
  });

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;
    const previousFocus = previousFocusRef.current;
    if (typeof dialog.showModal === "function") {
      if (!dialog.open) dialog.showModal();
    } else {
      dialog.setAttribute("open", "");
    }
    initialFocusRef?.current?.focus();
    return () => {
      if (typeof dialog.close === "function" && dialog.open) dialog.close();
      else dialog.removeAttribute("open");
      if (
        restoreFocus &&
        previousFocus instanceof HTMLElement &&
        previousFocus.isConnected
      ) {
        previousFocus.focus();
      }
    };
  }, [initialFocusRef, restoreFocus]);

  const containFocus = (event: KeyboardEvent<HTMLDialogElement>) => {
    layerKeyDown(event);
    event.stopPropagation();
    if (event.key === "Escape") return;
    if (event.key !== "Tab") return;

    const focusable = focusableElements(event.currentTarget);
    const first = focusable[0];
    const last = focusable.at(-1);
    if (!first || !last) {
      event.preventDefault();
      return;
    }
    if (
      event.shiftKey &&
      (document.activeElement === first ||
        !event.currentTarget.contains(document.activeElement))
    ) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  return createPortal(
    <dialog
      ref={dialogRef}
      className={["dirty-dialog", className].filter(Boolean).join(" ")}
      aria-modal="true"
      aria-labelledby={labelledBy}
      aria-describedby={describedBy}
      onCancel={(event) => {
        event.preventDefault();
        onDismiss();
      }}
      onKeyDown={containFocus}
    >
      <DismissLayerContext value={layer}>{children}</DismissLayerContext>
    </dialog>,
    document.body,
  );
}
