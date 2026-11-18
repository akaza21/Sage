import { errorMessage } from "../domain/errors";
import { useEditorSettingsStore } from "./editorSettingsStore";
import { currentProjectLayout } from "./projectProjection";
import { useWorkbenchStore } from "./store";
import { flushLiveWorkbench } from "./workbenchClose";

export function installWorkbenchNavigationGuard(): () => void {
  let flushing = false;
  let listening = false;
  let disposed = false;
  let layoutKey = JSON.stringify(
    currentProjectLayout(useWorkbenchStore.getState()),
  );
  let persistedLayoutKey = layoutKey;

  const pending = () => {
    const editor = useWorkbenchStore.getState().activeEditor;
    return (
      flushing ||
      layoutKey !== persistedLayoutKey ||
      useEditorSettingsStore.getState().persistencePhase === "saving" ||
      useEditorSettingsStore.getState().persistenceErrorKind === "save" ||
      (editor?.phase === "ready" && editor.revision > editor.persistedRevision)
    );
  };
  const beforeUnload = (event: BeforeUnloadEvent) => {
    if (!pending()) return;
    event.preventDefault();
    if (flushing) return;
    flushing = true;
    const epoch = useWorkbenchStore.getState().nativeTransitionEpoch;
    void flushLiveWorkbench(useWorkbenchStore.getState)
      .then((layout) => {
        const state = useWorkbenchStore.getState();
        if (
          state.nativeTransitionEpoch === epoch &&
          state.project?.id === layout?.projectId
        ) {
          persistedLayoutKey = JSON.stringify(layout);
        }
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
  const unsubscribe = useWorkbenchStore.subscribe((state, previous) => {
    if (
      state.project !== previous.project ||
      state.tabs !== previous.tabs ||
      state.activeTabId !== previous.activeTabId ||
      state.selectedRootId !== previous.selectedRootId ||
      state.selectedResource !== previous.selectedResource
    )
      layoutKey = JSON.stringify(currentProjectLayout(state));
    if (
      state.project?.id !== previous.project?.id ||
      state.nativeTransitionEpoch !== previous.nativeTransitionEpoch
    ) {
      persistedLayoutKey = layoutKey;
    }
    sync();
  });
  const unsubscribeSettings = useEditorSettingsStore.subscribe(sync);
  sync();
  return () => {
    disposed = true;
    unsubscribe();
    unsubscribeSettings();
    sync();
  };
}
