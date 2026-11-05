import { useEffect, type RefObject } from "react";

export function ComposerMenuLoading({
  triggerRef,
  onDismiss,
}: {
  triggerRef: RefObject<HTMLButtonElement | null>;
  onDismiss: () => void;
}) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" && event.key !== "Tab") return;
      onDismiss();
      if (event.key === "Escape") {
        event.preventDefault();
        triggerRef.current?.focus();
      }
    };
    const onPointerDown = (event: PointerEvent) => {
      if (!triggerRef.current?.contains(event.target as Node)) onDismiss();
    };
    document.addEventListener("keydown", onKeyDown);
    document.addEventListener("pointerdown", onPointerDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      document.removeEventListener("pointerdown", onPointerDown);
    };
  }, [onDismiss, triggerRef]);
  return (
    <span className="sr-only" role="status">
      Loading options…
    </span>
  );
}
