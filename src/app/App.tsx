import { useEffect, useState } from "react";

import { LazyModuleBoundary } from "../ui/LazyModuleBoundary";
import type { HarnessDefinition } from "../domain/workbench";
import { invokeNative } from "../ports/nativeInvoke";
import { desktop } from "../services/desktop";
import { useWorkbenchStore } from "./store";
import { traceCloseStage } from "./workspaceNotifyLifecycle";

const loadWorkbench = async () => {
  const { Workbench } = await import("../workbench/Workbench");
  return { default: Workbench };
};

const markStartup = (name: string) => {
  if (performance.getEntriesByName(name).length === 0) {
    performance.mark(name);
  }
};

const markHarnessPresence = (harnesses: HarnessDefinition[]) => {
  if (
    harnesses.some((harness) => harness.installState === "available") &&
    performance.getEntriesByName("sage-startup:harness-presence").length === 0
  ) {
    markStartup("sage-startup:harness-presence");
  }
};

export function App() {
  const [closeFailure, setCloseFailure] = useState<string | null>(null);
  const phase = useWorkbenchStore((state) => state.phase);
  const error = useWorkbenchStore((state) => state.error);
  const initialize = useWorkbenchStore((state) => state.initialize);

  useEffect(() => {
    markStartup("sage-startup:bootstrap-dispatch");
    void initialize().then(() => {
      markStartup("sage-startup:bootstrap-resolved");
      return useWorkbenchStore.getState().refreshRepository();
    });
  }, [initialize]);

  useEffect(() => {
    if (phase === "ready") markStartup("sage-startup:first-ready-render");
  }, [phase]);

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    markStartup("sage-startup:harness-subscription-start");
    void import("@tauri-apps/api/event").then(async ({ listen }) => {
      const removeListener = await listen<HarnessDefinition[]>(
        "harness-discovery-changed-v1",
        (event) => {
          markHarnessPresence(event.payload);
          void useWorkbenchStore
            .getState()
            .applyHarnessDiscovery(event.payload);
        },
      );
      markStartup("sage-startup:harness-subscription-ready");
      const harnesses = await desktop.listHarnesses();
      markHarnessPresence(harnesses);
      await useWorkbenchStore.getState().applyHarnessDiscovery(harnesses);
      if (disposed) removeListener();
      else unlisten = removeListener;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return;
    }
    let disposed = false;
    let closing = false;
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/event").then(async ({ listen }) => {
      const label = (
        window as unknown as {
          __TAURI_INTERNALS__: {
            metadata: { currentWindow: { label: string } };
          };
        }
      ).__TAURI_INTERNALS__.metadata.currentWindow.label;
      const removeListener = await listen(
        "tauri://close-requested",
        async () => {
          if (closing) {
            await invokeNative("plugin:window|destroy", { label });
            return;
          }
          traceCloseStage("closeRequested", "start");
          try {
            closing = true;
            setCloseFailure(null);
            await useWorkbenchStore.getState().closeWorkbench();
          } catch (error) {
            traceCloseStage("closeRequested", "error");
            setCloseFailure(
              error instanceof Error ? error.message : String(error),
            );
            closing = false;
          }
        },
        { target: { kind: "Window", label } },
      );
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

  let content;
  if (phase === "booting") {
    content = (
      <main className="launch-screen" aria-live="polite">
        <div className="brand-mark" aria-hidden="true">
          S
        </div>
        <p>Opening Sage...</p>
      </main>
    );
  } else if (phase === "error") {
    content = (
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
  } else {
    content = (
      <LazyModuleBoundary
        label="Workbench"
        load={loadWorkbench}
        render={(Workbench) => <Workbench />}
      />
    );
  }

  return (
    <>
      {closeFailure && (
        <div className="close-failure" role="alert">
          Sage could not close: {closeFailure}
        </div>
      )}
      {content}
    </>
  );
}
