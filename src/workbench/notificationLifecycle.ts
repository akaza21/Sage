import type {
  NotificationChat,
  NotificationPort,
} from "../ports/nativeNotificationPort";

export function observeNotificationVisibility(
  port: NotificationPort,
  chatIds: readonly string[],
  visible: boolean,
  source: Pick<
    Document,
    "visibilityState" | "addEventListener" | "removeEventListener"
  > = document,
): () => void {
  let disposed = false;
  let generation: number | null = null;
  let revision = 0;
  let previous: boolean | null = null;
  const ids = [...new Set(chatIds)].slice(0, 2);
  const publish = () => {
    if (generation === null) return;
    const documentVisible =
      !disposed && visible && source.visibilityState === "visible";
    if (previous === documentVisible) return;
    previous = documentVisible;
    void port
      .reportVisibility({
        generation,
        revision: ++revision,
        documentVisible,
        chatIds: documentVisible ? ids : [],
      })
      .catch(() => {
        // Superseded renderer generations are refused by native admission.
      });
  };
  source.addEventListener("visibilitychange", publish);
  void port
    .beginVisibility()
    .then((value) => {
      generation = value;
      publish();
    })
    .catch(() => {});
  return () => {
    disposed = true;
    source.removeEventListener("visibilitychange", publish);
    publish();
  };
}

export function listenNotificationNavigation(
  port: NotificationPort,
  ready: () => boolean,
  open: (chat: NotificationChat) => Promise<void>,
  onError: () => void,
) {
  let disposed = false;
  let running = false;
  let retained: NotificationChat | null = null;
  let pending = true;
  let unlisten: (() => void) | undefined;
  const wake = () => {
    pending = true;
    if (disposed || running || !ready()) return;
    running = true;
    void (async () => {
      try {
        while (!disposed && pending && ready()) {
          pending = false;
          retained ??= await port.takeChat();
          if (disposed) {
            retained = null;
            break;
          }
          if (!ready()) {
            pending = true;
            break;
          }
          const chat = retained;
          retained = null;
          if (chat) await open(chat);
        }
      } catch {
        if (!disposed) onError();
      } finally {
        running = false;
      }
    })();
  };
  void port
    .listenOpen(wake)
    .then((stop) => {
      if (disposed) stop();
      else {
        unlisten = stop;
        wake();
      }
    })
    .catch(() => {
      if (!disposed) onError();
    });
  return {
    wake,
    dispose() {
      disposed = true;
      retained = null;
      unlisten?.();
    },
  };
}
