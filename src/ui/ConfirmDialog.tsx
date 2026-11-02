import { useId, useRef } from "react";

import { ModalDialog } from "./ModalDialog";

export function ConfirmDialog({
  title,
  description,
  confirmLabel,
  onConfirm,
  onCancel,
}: {
  title: string;
  description: string;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const id = useId();
  const cancelRef = useRef<HTMLButtonElement>(null);

  return (
    <ModalDialog
      labelledBy={`${id}-title`}
      describedBy={`${id}-description`}
      initialFocusRef={cancelRef}
      restoreFocus
      onDismiss={onCancel}
    >
      <h2 id={`${id}-title`}>{title}</h2>
      <p id={`${id}-description`}>{description}</p>
      <div className="dirty-dialog__actions">
        <button
          ref={cancelRef}
          className="button"
          type="button"
          onClick={onCancel}
        >
          Cancel
        </button>
        <button
          className="button button--danger"
          type="button"
          onClick={onConfirm}
        >
          {confirmLabel}
        </button>
      </div>
    </ModalDialog>
  );
}
