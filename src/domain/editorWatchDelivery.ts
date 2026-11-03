import type {
  EditorExternalWatchNotice,
  EditorWatchDelivery,
} from "./editorWatch";

export function createInjectedEditorWatchDelivery(): {
  delivery: EditorWatchDelivery;
  emit(notice: EditorExternalWatchNotice): void;
  close(): void;
} {
  const listeners = new Set<(notice: EditorExternalWatchNotice) => void>();
  let open = true;
  return {
    delivery: {
      subscribe(listener) {
        if (!open) {
          return () => undefined;
        }
        listeners.add(listener);
        return () => {
          listeners.delete(listener);
        };
      },
    },
    emit(notice) {
      if (!open) {
        return;
      }
      for (const listener of listeners) {
        listener(notice);
      }
    },
    close() {
      open = false;
      listeners.clear();
    },
  };
}
