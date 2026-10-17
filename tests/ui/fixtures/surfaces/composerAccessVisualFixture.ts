import { useWorkbenchStore } from "../../../../src/app/store";
import { useSessionAccessModeStore } from "../../../../src/app/sessionAccessModeStore";
import type { SessionAccessModeCapability } from "../../../../src/domain/sessionAccessMode";

export function installVisualComposerAccessFixture(
  accessModes: SessionAccessModeCapability[],
) {
  const state = useWorkbenchStore.getState();
  const chat = state.tabs.find((tab) => tab.kind === "chat");
  if (!chat) throw new Error("Access fixture requires a Chat");
  const harnessId =
    state.chatUiById[chat.id]?.harnessId ?? state.selectedHarnessId;
  useWorkbenchStore.setState({
    harnesses: state.harnesses.map((harness) =>
      harness.id === harnessId ? { ...harness, accessModes } : harness,
    ),
  });
  useSessionAccessModeStore.setState({
    modes: {},
    ready: true,
    error: null,
  });
}
