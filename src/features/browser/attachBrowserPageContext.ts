import { pinnedAgentChatOwner } from "../../app/agentChatOwner";
import { useWorkbenchStore } from "../../app/store";
import { parseBrowserPageContext } from "../../domain/attachedContext";
import type { ChatOwnershipIntent } from "../../domain/chatOwnership";
import {
  browserStatus,
  browserPageContext,
} from "../../ports/nativeBrowserPort";

export async function attachBrowserPageContext(
  tabId: string,
  focusedChatId: string | null,
  intent: ChatOwnershipIntent,
) {
  const before = await browserStatus(tabId);
  if (useWorkbenchStore.getState().activeTabId !== tabId)
    throw new Error("Browser page or focused chat changed; try again.");
  if (before.loading || before.error || !before.url)
    throw new Error("Browser page unavailable; try again.");
  const captured = parseBrowserPageContext(
    await browserPageContext(tabId, before.revision),
  );
  if (useWorkbenchStore.getState().activeTabId !== tabId)
    throw new Error("Browser page or focused chat changed; try again.");
  const current = await browserStatus(tabId);
  if (
    !captured ||
    captured.tabId !== tabId ||
    current.loading ||
    current.error ||
    current.url !== before.url ||
    current.revision !== before.revision ||
    useWorkbenchStore.getState().activeTabId !== tabId ||
    pinnedAgentChatOwner() !== focusedChatId
  )
    throw new Error("Browser page or focused chat changed; try again.");
  if (!useWorkbenchStore.getState().attachChatContext(intent, captured)) {
    throw new Error("Could not attach the page to the focused chat.");
  }
}
