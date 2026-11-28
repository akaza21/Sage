import { createNativeWorkbenchPorts } from "../../../src/ports/nativeWorkbenchPorts";
import { isTauriRuntime } from "../../../src/ports/nativeInvoke";
import type { WorkbenchPorts } from "../../../src/ports/workbenchPorts";
import { createBrowserWorkbenchPorts } from "./browserWorkbenchPorts";

// Test-only stand-in for src/ports/workbenchRuntime.ts. Vite resolves the app's
// runtime import here only in the `ui-fixture` and `performance-browser` modes
// and under Vitest. A page with a fake Tauri IPC surface still reaches the
// native adapter; a plain page gets the in-page simulator.
let ports: WorkbenchPorts | null = null;

// The simulator stands in for a backend, so the app always mounts.
export function hasWorkbenchBackend(): boolean {
  return true;
}

export function getWorkbenchPorts(): WorkbenchPorts {
  if (!ports) {
    ports = isTauriRuntime()
      ? createNativeWorkbenchPorts()
      : createBrowserWorkbenchPorts();
  }
  return ports;
}

export function resetWorkbenchPortsForTests(): void {
  ports = null;
}
