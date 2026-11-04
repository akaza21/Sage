import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import {
  seedSnapshotOwner,
  snapshotRef,
  snapshotResource,
} from "../app/agentDiff.fixture";
import { clearAcpProjectionRegistry } from "../app/acpProjectionRegistry";
import { useAgentArtifactAdmissionStore } from "../app/agentArtifactAdmissionStore";
import { useAgentArtifactFollowStore } from "../app/agentArtifactFollowStore";
import { useSubagentPaneStore } from "../app/subagentPaneStore";
import { useWorkbenchStore } from "../app/store";
import { useAgentChangesState } from "./agentChangesState";
import { AgentToolPanel } from "./AgentToolPanel";

const initial = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  clearAcpProjectionRegistry();
  useAgentArtifactAdmissionStore.getState().clear();
  useAgentArtifactFollowStore.getState().clear();
  useSubagentPaneStore.getState().clear();
  useAgentChangesState.getState().close();
  useWorkbenchStore.setState(initial, true);
});

it.each(["reported resource", "already admitted tab"])(
  "selects a Files %s over Changes and a previously selected child",
  async (kind) => {
    seedSnapshotOwner();
    const resource =
      kind === "reported resource"
        ? snapshotResource
        : { rootId: snapshotResource.rootId, relativePath: "other.ts" };
    const tab = {
      id: "file-selected",
      kind: "file" as const,
      title: resource.relativePath,
      position: 1,
      pinned: false,
      dirty: false,
      resource,
    };
    const openFile = vi.fn(async () => {
      useWorkbenchStore.setState({ tabs: [tab], activeTabId: tab.id });
    });
    const activateTab = vi.fn(async () => {
      useWorkbenchStore.setState({ activeTabId: tab.id });
    });
    useWorkbenchStore.setState({
      openFile,
      activateTab,
      tabs: kind === "already admitted tab" ? [tab] : [],
    });
    if (kind === "already admitted tab")
      useAgentArtifactAdmissionStore
        .getState()
        .admit({ owner: snapshotRef, tabId: tab.id });
    useAgentChangesState.getState().show(snapshotRef, 1);
    useSubagentPaneStore
      .getState()
      .show({ owner: snapshotRef, itemId: "child", agentKey: "child" }, true);
    const dismiss = vi.fn();
    render(
      <AgentToolPanel
        request={{
          tool: "files",
          chatId: snapshotRef.chatId,
          projectId: snapshotRef.projectId,
          generation: 1,
        }}
        onDismiss={dismiss}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: resource.relativePath }),
    );
    await waitFor(() => expect(dismiss).toHaveBeenCalledOnce());
    expect(useAgentChangesState.getState().selected).toBe(false);
    expect(useSubagentPaneStore.getState().selected).toBe(false);
    expect(
      kind === "reported resource" ? openFile : activateTab,
    ).toHaveBeenCalledOnce();
  },
);
