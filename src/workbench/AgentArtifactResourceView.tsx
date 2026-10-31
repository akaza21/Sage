import type { ComponentProps, ReactNode, Ref } from "react";
import type { WorkbenchTab } from "../domain/workbench";
import { useRetainedElement } from "../ui/useRetainedElement";
import { AgentArtifactHeader } from "./AgentArtifactHeader";
import { AgentArtifactFileHeaderBridge } from "./useAgentArtifactHeaderSlot";
import {
  renderFileOrDiffCanvas,
  renderRegisteredCanvas,
} from "./surfaceRegistry";

/** One resource bar and its shared workbench canvas, loaded only with resources. */
export function AgentArtifactResourceView({
  tab,
  showResource,
  header,
  panelId,
  canvasRef,
  children,
}: {
  tab: WorkbenchTab | null;
  showResource: boolean;
  header: Omit<
    ComponentProps<typeof AgentArtifactHeader>,
    "tab" | "fileActionsRef" | "fileDetailsRef"
  >;
  panelId: string;
  canvasRef: Ref<HTMLDivElement>;
  children: ReactNode;
}) {
  const [actions, setActions] = useRetainedElement<HTMLDivElement>();
  const [details, setDetails] = useRetainedElement<HTMLDivElement>();
  return (
    <>
      {tab && tab.kind !== "browser" ? (
        <AgentArtifactHeader
          key={tab.id}
          tab={tab}
          {...header}
          fileActionsRef={setActions}
          fileDetailsRef={setDetails}
        />
      ) : null}
      <div
        id={panelId}
        role="tabpanel"
        aria-label="Selected resource"
        ref={canvasRef}
        className="agent-artifact-pane__canvas"
      >
        {children}
        <AgentArtifactFileHeaderBridge actions={actions} details={details}>
          {showResource && tab
            ? tab.kind === "browser"
              ? renderRegisteredCanvas("browser", tab)
              : renderFileOrDiffCanvas(tab, { readOnly: true })
            : null}
        </AgentArtifactFileHeaderBridge>
      </div>
    </>
  );
}
