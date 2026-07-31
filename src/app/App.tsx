import { useEffect } from "react";

import { Workbench } from "../workbench/Workbench";
import { useWorkbenchStore } from "./store";

export function App() {
  const phase = useWorkbenchStore((state) => state.phase);
  const error = useWorkbenchStore((state) => state.error);
  const initialize = useWorkbenchStore((state) => state.initialize);

  useEffect(() => {
    void initialize();
  }, [initialize]);

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return;
    }
    let disposed = false;
    let closing = false;
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/window").then(async ({ getCurrentWindow }) => {
      const appWindow = getCurrentWindow();
      const removeListener = await appWindow.onCloseRequested(async (event) => {
        if (closing) {
          return;
        }
        event.preventDefault();
        try {
          closing = true;
          await useWorkbenchStore.getState().closeWorkbench();
        } catch {
          closing = false;
        }
      });
      if (disposed) {
        removeListener();
      } else {
        unlisten = removeListener;
      }
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  if (phase === "booting") {
    return (
      <main className="launch-screen" aria-live="polite">
        <div className="brand-mark" aria-hidden="true">
          S
        </div>
        <p>Opening Sage...</p>
      </main>
    );
  }

  if (phase === "error") {
    return (
      <main className="launch-screen">
        <div className="brand-mark brand-mark--error" aria-hidden="true">
          !
        </div>
        <h1>Sage could not open</h1>
        <p>{error ?? "An unknown startup error occurred."}</p>
        <button
          className="button"
          type="button"
          onClick={() => void initialize()}
        >
          Try again
        </button>
      </main>
    );
  }

  return <Workbench />;
}
