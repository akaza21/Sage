import { useEffect, type RefObject } from "react";

import { useDismissLayer } from "../../ui/dismissLayer";

// The trigger gave up focus on press, so Escape is taken from the whole
// document, before the panes around the composer can act on it.
export function ComposerMenuLoading({
  triggerRef,
  onDismiss,
}: {
  triggerRef: RefObject<HTMLButtonElement | null>;
  onDismiss: () => void;
}) {
  useDismissLayer(true, { inside: [triggerRef], onOutside: onDismiss });
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" && event.key !== "Tab") return;
      onDismiss();
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        triggerRef.current?.focus();
      }
    };
    document.addEventListener("keydown", onKeyDown, true);
    return () => document.removeEventListener("keydown", onKeyDown, true);
  }, [onDismiss, triggerRef]);
  return (
    <span className="sr-only" role="status">
      Loading options…
    </span>
  );
}
