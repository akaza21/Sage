import {
  applyBrowseIntent,
  harnessBrowseOwner,
} from "../../../src/features/preview/harnessBrowse";
import { installVisualAgentLongPathFixture } from "./surfaces/resourceVisualFixtures";
import { pinnedAgentChatOwner } from "../../../src/app/agentChatOwner";
import { acpProjectionStoreFor } from "../../../src/app/acpProjectionRegistry";
import { useAgentArtifactAdmissionStore } from "../../../src/app/agentArtifactAdmissionStore";
import { useAgentArtifactFollowStore } from "../../../src/app/agentArtifactFollowStore";
import { useWorkbenchStore } from "../../../src/app/store";

export async function installCp27bResource(kind: "file" | "diff" | "preview") {
  if (kind !== "preview") {
    const chatId = pinnedAgentChatOwner();
    const page = acpProjectionStoreFor(chatId).getState().page;
    if (!page?.currentSessionId)
      throw new Error("CP-27b file fixture requires the focused chat session");
    const owner = {
      projectId: page.projectId,
      chatId: page.chatId,
      sessionId: page.currentSessionId,
    };
    await installVisualAgentLongPathFixture();
    const tabs = useWorkbenchStore
      .getState()
      .tabs.filter(
        (tab) =>
          tab.id === "visual-long-file-tab" ||
          tab.id === "visual-long-diff-tab",
      );
    for (const tab of tabs)
      useAgentArtifactAdmissionStore.getState().admit({ owner, tabId: tab.id });
    const selected = tabs.find((tab) => tab.kind === kind);
    if (!selected)
      throw new Error("CP-27b file fixture did not install its resource");
    useAgentArtifactFollowStore
      .getState()
      .pin(owner, { kind: "tab", tabId: selected.id });
    return;
  }
  const owner = harnessBrowseOwner();
  if (
    !owner ||
    !applyBrowseIntent({
      owner,
      url: `${location.origin}/tests/ui/fixtures/preview.html`,
    })
  ) {
    throw new Error("CP-27b Preview fixture requires the focused chat owner");
  }
}
