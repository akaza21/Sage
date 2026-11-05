import { useWorkbenchStore } from "../app/store";
import { isAgentArtifactTabKind } from "./workbenchWindowMode";
import {
  Activity,
  memo,
  useState,
  useCallback,
  type ComponentProps,
} from "react";
import AgentChatRuntime from "./AgentChatRuntime";
import { AgentChatRuntimeContext } from "./AgentChatRuntimeContext";
import BodyView from "./AgentWorkbenchBodyView";
const Presentation = memo(BodyView);

/** Runtime ownership survives while the Agent workspace's presentation sleeps. */
export default function AgentWorkbenchHost({
  visible = true,
  onEnterEditor,
  ...props
}: Omit<ComponentProps<typeof BodyView>, "onOpenArtifactInEditor"> & {
  visible?: boolean;
  onEnterEditor: () => void;
}) {
  const [ids, setIds] = useState<readonly string[]>([]);
  const openArtifactInEditor = useCallback(
    async (tabId: string) => {
      const before = useWorkbenchStore.getState();
      if (
        before.projectTransitioning ||
        !before.tabs.some(
          (tab) => tab.id === tabId && isAgentArtifactTabKind(tab.kind),
        )
      )
        return;
      await before.activateTab(tabId);
      const after = useWorkbenchStore.getState();
      if (
        after.projectTransitioning ||
        after.projectGeneration !== before.projectGeneration ||
        after.activeTabId !== tabId
      )
        return;
      onEnterEditor();
    },
    [onEnterEditor],
  );

  return (
    <AgentChatRuntimeContext value={setIds}>
      <AgentChatRuntime ids={ids} visible={visible} />
      <Activity mode={visible ? "visible" : "hidden"}>
        <Presentation
          {...props}
          onOpenArtifactInEditor={openArtifactInEditor}
        />
      </Activity>
    </AgentChatRuntimeContext>
  );
}
