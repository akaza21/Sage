import { useId, useRef } from "react";
import { ModalDialog } from "../ui/ModalDialog";

export function SmartCommitPrompt({
  onConfirm,
  onCancel,
}: {
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const id = useId();
  const cancel = useRef<HTMLButtonElement>(null);
  return (
    <ModalDialog
      className="dirty-dialog--editor"
      labelledBy={id}
      initialFocusRef={cancel}
      restoreFocus
      onDismiss={onCancel}
    >
      <p id={id}>
        There are no staged changes to commit. Stage all your changes and commit
        them directly?
      </p>
      <div className="dirty-dialog__actions">
        <button
          className="button"
          type="button"
          ref={cancel}
          onClick={onCancel}
        >
          No
        </button>
        <button className="button" type="button" onClick={onConfirm}>
          Yes
        </button>
      </div>
    </ModalDialog>
  );
}
