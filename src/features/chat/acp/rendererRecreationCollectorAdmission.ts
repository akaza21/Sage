import type { HarnessDefinition } from "../../../domain/harness";
import type { WorkbenchWindowMode } from "../../../workbench/workbenchWindowMode";

export interface RendererRecreationCollectorCandidate {
  windowMode: WorkbenchWindowMode;
  activeTabId: string | null;
  focusedChatId: string | null;
  harness: HarnessDefinition | undefined;
}

export function admitsFocusedCodexRendererRecreation(
  candidate: RendererRecreationCollectorCandidate,
): boolean {
  const { harness } = candidate;
  return Boolean(
    candidate.windowMode === "agent" &&
    candidate.activeTabId &&
    candidate.activeTabId === candidate.focusedChatId &&
    harness?.id === "codex" &&
    harness.transport === "acp" &&
    harness.source !== "development" &&
    harness.installState === "available" &&
    harness.bindingId === "codex-app-server",
  );
}
