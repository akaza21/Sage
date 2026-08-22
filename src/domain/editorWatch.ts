import { sameResource } from "./resourceIdentity";
import type { ResourceRef } from "./resource";

export const EDITOR_EXTERNAL_WATCH_EVENT = "editor-external-watch-v1";

export type EditorExternalWatchKind = "changed" | "deleted";

export interface EditorExternalWatchNotice {
  resource: ResourceRef;
  generation: number;
  kind: EditorExternalWatchKind;
}

export interface EditorWatchDelivery {
  subscribe(listener: (notice: EditorExternalWatchNotice) => void): () => void;
}

export interface EditorWatchSubscription {
  resource: ResourceRef;
  cancel(): Promise<void>;
  delivery: EditorWatchDelivery;
}

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

export function noticeMatchesWatch(
  notice: EditorExternalWatchNotice,
  resource: ResourceRef,
): boolean {
  return sameResource(notice.resource, resource);
}
