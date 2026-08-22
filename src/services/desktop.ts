import { getWorkbenchPorts } from "../ports/workbenchRuntime";

function createDesktop() {
  const ports = getWorkbenchPorts();
  return {
    ...ports.project,
    ...ports.editor,
    ...ports.repository,
    ...ports.chat,
    ...ports.agent,
  };
}

export const desktop = createDesktop();
