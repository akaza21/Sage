import { useRef, useState } from "react";

import { useWorkbenchStore } from "../app/store";
import { ModalDialog } from "../ui/ModalDialog";

type PendingCloseAction = "discard" | "save";

function DirtyTabConfirmation({
  tabId,
  title,
  cancel,
  discard,
  save,
}: {
  tabId: string;
  title: string;
  cancel: () => void;
  discard: () => Promise<void>;
  save: () => Promise<void>;
}) {
  const [pendingAction, setPendingAction] = useState<PendingCloseAction | null>(
    null,
  );
  const pendingActionRef = useRef<PendingCloseAction | null>(null);
  const pending = pendingAction !== null;

  const dismiss = () => {
    if (
      pendingActionRef.current ||
      useWorkbenchStore.getState().pendingCloseTabId !== tabId
    ) {
      return;
    }
    cancel();
  };

  const run = async (
    action: PendingCloseAction,
    operation: () => Promise<void>,
  ) => {
    if (
      pendingActionRef.current ||
      useWorkbenchStore.getState().pendingCloseTabId !== tabId
    ) {
      return;
    }
    pendingActionRef.current = action;
    setPendingAction(action);
    try {
      await operation();
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
          onClick={() => void run("discard", discard)}
        >
          {pendingAction === "discard" ? "Discarding…" : "Discard"}
        </button>
        <button
          className="button button--primary"
          type="button"
          disabled={pending}
          onClick={() => void run("save", save)}
        >
          {pendingAction === "save" ? "Saving…" : "Save"}
        </button>
      </div>
    </ModalDialog>
  );
}

export function DirtyTabDialog() {
  const pendingCloseTabId = useWorkbenchStore(
    (state) => state.pendingCloseTabId,
  );
  const tabs = useWorkbenchStore((state) => state.tabs);
  const cancelCloseTab = useWorkbenchStore((state) => state.cancelCloseTab);
  const saveAndCloseTab = useWorkbenchStore((state) => state.saveAndCloseTab);
  const discardAndCloseTab = useWorkbenchStore(
    (state) => state.discardAndCloseTab,
  );
  const tab = tabs.find((candidate) => candidate.id === pendingCloseTabId);

  if (!tab) {
    return null;
  }

  return (
    <DirtyTabConfirmation
      key={tab.id}
      tabId={tab.id}
      title={tab.title}
      cancel={cancelCloseTab}
      discard={discardAndCloseTab}
      save={saveAndCloseTab}
    />
  );
}
