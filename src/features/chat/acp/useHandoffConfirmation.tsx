import { useCallback, useEffect, useId, useRef, useState } from "react";
import { ModalDialog } from "../../../ui/ModalDialog";

export function useHandoffConfirmation(scope: string) {
  const id = useId();
  // Keyed by scope: a scope change closes the dialog without an effect write.
  const [openScope, setOpenScope] = useState<string | null>(null);
  const open = openScope === scope;
  const pending = useRef<((confirmed: boolean) => void) | null>(null);
  const focus = useRef<HTMLElement | null>(null);
  const active = useRef<string | null>(null);
  const restoreFocus = useRef(false);
  const settle = useCallback((confirmed: boolean) => {
    const resolve = pending.current;
    pending.current = null;
    setOpenScope(null);
    restoreFocus.current = true;
    resolve?.(confirmed);
  }, []);
  useEffect(() => {
    active.current = scope;
    return () => {
      active.current = null;
      restoreFocus.current = false;
      pending.current?.(false);
      pending.current = null;
      setOpenScope(null);
    };
  }, [scope]);
  useEffect(() => {
    if (!open && restoreFocus.current) {
      restoreFocus.current = false;
      focus.current?.focus();
    }
  }, [open]);
  const request = useCallback(() => {
    if (active.current !== scope) return Promise.resolve(false);
    pending.current?.(false);
    focus.current =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    setOpenScope(scope);
    return new Promise<boolean>((resolve) => {
      pending.current = resolve;
    });
  }, [scope]);
  return {
    request,
    dialog: open ? (
      <ModalDialog
        className="dialog--scrim"
        labelledBy={id}
        onDismiss={() => settle(false)}
      >
        <h2 id={id}>Retry with a fresh session?</h2>
        <p>
          Prior execution may have occurred. Replace the native session and
          explicitly retry this request?
        </p>
        <div className="dirty-dialog__actions">
          <button
            autoFocus
            className="button--compact"
            type="button"
            onClick={() => settle(false)}
          >
            Cancel
          </button>
          <button
            className="button--compact"
            type="button"
            onClick={() => settle(true)}
          >
            Replace and retry
          </button>
        </div>
      </ModalDialog>
    ) : null,
  };
}
