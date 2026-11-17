import { errorMessage } from "../domain/errors";
import { useWorkbenchStore } from "./store";

export function installWorkbenchNavigationGuard(): () => void {
  let flushing = false;
  let layoutPending = false;
  let listening = false;
  let disposed = false;

  const pending = () => {
    const editor = useWorkbenchStore.getState().activeEditor;
    return (
      layoutPending ||
      (editor?.phase === "ready" && editor.revision > editor.persistedRevision)
    );
  };
  const beforeUnload = (event: BeforeUnloadEvent) => {
    if (!pending()) return;
    event.preventDefault();
    if (flushing) return;
    flushing = true;
    layoutPending = true;
    void useWorkbenchStore
      .getState()
      .persistLayout()
      .then(() => {
        layoutPending = false;
      })
      .catch((error: unknown) => {
        useWorkbenchStore.setState({ error: errorMessage(error) });
      })
      .finally(() => {
        flushing = false;
        sync();
      });
  };
  const sync = () => {
    const needed = !disposed && pending();
    if (needed === listening) return;
    listening = needed;
    if (needed) window.addEventListener("beforeunload", beforeUnload);
    else window.removeEventListener("beforeunload", beforeUnload);
  };
  const unsubscribe = useWorkbenchStore.subscribe(sync);
  sync();
  return () => {
    disposed = true;
    unsubscribe();
    sync();
  };
}
