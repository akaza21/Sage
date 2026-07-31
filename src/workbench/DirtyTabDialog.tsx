import { useWorkbenchStore } from "../app/store";

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
    <div className="dialog-backdrop">
      <section
        className="dirty-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="dirty-dialog-title"
      >
        <h2 id="dirty-dialog-title">Save changes to {tab.title}?</h2>
        <p>
          The buffer is restart-safe, but closing it requires an explicit
          decision.
        </p>
        <div className="dirty-dialog__actions">
          <button className="button" type="button" onClick={cancelCloseTab}>
            Cancel
          </button>
          <button
            className="button button--danger"
            type="button"
            onClick={() => void discardAndCloseTab()}
          >
            Discard
          </button>
          <button
            className="button button--primary"
            type="button"
            onClick={() => void saveAndCloseTab()}
          >
            Save
          </button>
        </div>
      </section>
    </div>
  );
}
