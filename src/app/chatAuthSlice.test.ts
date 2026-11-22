import { afterEach, beforeEach, expect, it } from "vitest";
import type { SageTurnId } from "../domain/acp";
import { useWorkbenchStore } from "./store";

const initial = useWorkbenchStore.getState();
const turnId = "turn-auth" as SageTurnId;

beforeEach(() => {
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "A", roots: [] },
    tabs: ["chat-a", "chat-b"].map((id) => ({
      id,
      kind: "chat",
      title: id,
      position: 0,
      pinned: false,
      dirty: false,
      status: "idle",
    })),
    activeTabId: "chat-b",
    composerText: "Draft B",
    chatUiById: {},
    chatOwnershipEpoch: 1,
  });
});

afterEach(() => useWorkbenchStore.setState(initial, true));

it("dismisses only the owning chat's turn without replacing its draft or the active chat", () => {
  const state = useWorkbenchStore.getState();
  const owner = state.captureChatOwnership("chat-a")!;
  state.setChatComposer(owner, "Draft A");
  state.dismissChatAuthTurn(owner, turnId);
  const updated = useWorkbenchStore.getState();
  expect(updated.chatUiById["chat-a"]).toMatchObject({
    composerText: "Draft A",
    dismissedAuthTurnId: turnId,
  });
  expect(updated.chatUiById["chat-b"]).toBeUndefined();
  expect(updated.activeTabId).toBe("chat-b");
  expect(updated.composerText).toBe("Draft B");
});

it("ignores recovery after its project ownership expires", () => {
  const owner = useWorkbenchStore.getState().captureChatOwnership("chat-a")!;
  useWorkbenchStore.setState({ chatOwnershipEpoch: 2 });
  useWorkbenchStore.getState().dismissChatAuthTurn(owner, turnId);
  expect(useWorkbenchStore.getState().chatUiById).toEqual({});
});
