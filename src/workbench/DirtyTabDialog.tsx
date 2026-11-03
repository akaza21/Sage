import { useRef, useState } from "react";

import { useWorkbenchStore } from "../app/store";
import { ModalDialog } from "../ui/ModalDialog";

type PendingCloseAction = "discard" | "save";

function DirtyTabConfirmation({
  tabId,
  title,
}: {
  tabId: string;
  title: string;
}) {
  const [pendingAction, setPendingAction] = useState<PendingCloseAction | null>(
    null,
  );
  const pendingActionRef = useRef<PendingCloseAction | null>(null);
  const pending = pendingAction !== null;

  const activeCommands = () => {
    const state = useWorkbenchStore.getState();
    return !pendingActionRef.current && state.pendingCloseTabId === tabId
      ? state
      : null;
  };
  const dismiss = () => activeCommands()?.cancelCloseTab();
  const run = async (action: PendingCloseAction) => {
    const commands = activeCommands();
    if (!commands) return;
    pendingActionRef.current = action;
    setPendingAction(action);
    try {
      await (action === "save"
        ? commands.saveAndCloseTab()
        : commands.discardAndCloseTab());
    } finally {
      if (useWorkbenchStore.getState().pendingCloseTabId === tabId) {
        pendingActionRef.current = null;
        setPendingAction(null);
      }
    }
  };

  return (
    <ModalDialog
      className="dirty-dialog--editor"
      labelledBy="dirty-dialog-title"
      onDismiss={dismiss}
    >
      <h2 id="dirty-dialog-title">Save changes to {title}?</h2>
      <p>
        The buffer is restart-safe, but closing it requires an explicit
        decision.
      </p>
      <div className="dirty-dialog__actions">
        <button
          className="button"
          type="button"
          disabled={pending}
          onClick={dismiss}
        >
          Cancel
        </button>
        <button
          className="button button--danger"
          type="button"
          disabled={pending}
          onClick={() => void run("discard")}
        >
          {pendingAction === "discard" ? "Discarding…" : "Discard"}
        </button>
        <button
          className="button button--primary"
          type="button"
          disabled={pending}
          onClick={() => void run("save")}
        >
          {pendingAction === "save" ? "Saving…" : "Save"}
        </button>
      </div>
    </ModalDialog>
  );
}

export function DirtyTabDialog() {
  const tab = useWorkbenchStore((state) =>
    state.tabs.find((candidate) => candidate.id === state.pendingCloseTabId),
  );

  if (!tab) {
    return null;
  }

  return <DirtyTabConfirmation key={tab.id} tabId={tab.id} title={tab.title} />;
}
