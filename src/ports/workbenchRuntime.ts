import { createBrowserWorkbenchPorts } from "./browserWorkbenchPorts";
import { createNativeWorkbenchPorts } from "./nativeWorkbenchPorts";
import { isTauriRuntime } from "./nativeInvoke";
import type { WorkbenchPorts } from "./workbenchPorts";

let ports: WorkbenchPorts | null = null;

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
