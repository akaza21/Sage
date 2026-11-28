import { useEffect } from "react";

/**
 * While the auto-hidden panel floats over the chat, a press anywhere outside
 * it (except the title bar, which holds its toggle) or Escape puts it away.
 */
export function useNavigationOverlayDismiss(
  open: boolean,
  dismiss: () => void,
) {
  useEffect(() => {
    if (!open) return;
    const away = (event: Event) => {
      if (
        event instanceof KeyboardEvent
          ? event.key === "Escape"
          : !(event.target as Element).closest?.(".project-sidebar, .title-bar")
      )
        dismiss();
    };
    document.addEventListener("pointerdown", away, true);
    document.addEventListener("keydown", away);
    return () => {
      document.removeEventListener("pointerdown", away, true);
      document.removeEventListener("keydown", away);
    };
  }, [open, dismiss]);
}
