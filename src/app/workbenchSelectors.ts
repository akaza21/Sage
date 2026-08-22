import type { EditorDocument, ProjectSummary } from "../domain/workbench";
import type { WorkbenchState } from "./store";

export function selectOpenProject(
  state: WorkbenchState,
): ProjectSummary | null {
  return state.project;
}

export function selectRepositoryStatus(state: WorkbenchState) {
  return state.repositories;
}

export function selectActiveChatId(state: WorkbenchState): string | null {
  const activeTab =
    state.tabs.find((tab) => tab.id === state.activeTabId) ?? null;
  return activeTab?.kind === "chat" ? activeTab.id : null;
}

export function selectAgentSession(state: WorkbenchState): {
  sessionId: string | null;
  modelId: string | null;
} {
  return {
    sessionId: state.sessionId,
    modelId: state.selectedModelId,
  };
}

export function selectEditorDocument(
  state: WorkbenchState,
): EditorDocument | null {
  const editor = state.activeEditor;
  return editor?.phase === "ready" ? editor.document : null;
}
