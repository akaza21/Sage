import { useWorkbenchStore } from "../app/store";

/** Recovery and browser-data notices; the status bar shows them in Editor,
 *  the toolbar row in Agent. */
export function WorkbenchNotices() {
  const recoveryNotice = useWorkbenchStore((state) => state.recoveryNotice);
  const browserClearNotice = useWorkbenchStore(
    (state) => state.browserClearNotice,
  );
  return (
    <>
      {recoveryNotice ? <span role="status">{recoveryNotice}</span> : null}
      {browserClearNotice ? (
        <span role="status">{browserClearNotice}</span>
      ) : null}
    </>
  );
}
