import { useState, type CSSProperties } from "react";

import { AgentArtifactPane } from "./AgentArtifactPane";
import { AgentChatPanes } from "./AgentChatPanes";
import { LeftSidebar } from "./LeftSidebar";
import { AgentNavigationPart } from "./agentNavigationLazy";
import { AgentToolsProvider } from "./AgentToolsProvider";
import { WorkbenchSidebarSplitter } from "./WorkbenchSidebarSplitter";
import type { agentWorkbenchLayout } from "./agentWorkbenchLayout";
import { useAgentSettingsState } from "../features/settings/agentSettingsState";

export function AgentWorkbenchBody({
  layout,
  style,
  drawerOpen,
  navigationCollapsed = false,
  onExpandNavigation,
  artifactExpanded,
  onArtifactExpandedChange,
  onAgentLeftRailWidthChange,
  onAgentArtifactWidthChange,
  onOpenArtifactInEditor,
  onOpenTerminal,
}: {
  layout: ReturnType<typeof agentWorkbenchLayout>;
  style: CSSProperties | undefined;
  drawerOpen: boolean;
  navigationCollapsed?: boolean;
  onExpandNavigation: () => void;
  artifactExpanded: boolean;
  onArtifactExpandedChange: (expanded: boolean) => void;
  onAgentLeftRailWidthChange: (width: number) => void;
  onAgentArtifactWidthChange: (width: number) => void;
  onOpenArtifactInEditor: (tabId: string) => void;
  onOpenTerminal?: () => void;
}) {
  const openSettings = useAgentSettingsState((state) => state.open);
  const [artifactHeader, setArtifactHeader] = useState<HTMLDivElement | null>(
    null,
  );
  return (
    <AgentToolsProvider onOpenTerminal={onOpenTerminal}>
      <div
        className={`workbench__body${layout.artifactOverlay ? " workbench__body--artifact-overlay" : ""}`}
        style={
          {
            ...style,
            "--agent-artifact-overlay-width": `${layout.artifactOverlayWidth}px`,
          } as CSSProperties
        }
      >
        {navigationCollapsed ? (
          <AgentNavigationPart
            part="AgentCollapsedNavigation"
            inert={layout.artifactModal}
            onOpenSettings={() => openSettings()}
            onExpand={onExpandNavigation}
          />
        ) : (
          <LeftSidebar inert={layout.artifactModal} />
        )}
        {layout.navigationResizable ? (
          <WorkbenchSidebarSplitter
            ariaLabel="Resize project and chat rail"
            side="leading"
            value={layout.leftRailWidth}
            width={layout.leftRailBounds}
            onChange={onAgentLeftRailWidthChange}
          />
        ) : null}
        <div
          className="center-region"
          inert={drawerOpen}
          aria-hidden={drawerOpen || undefined}
        >
          <AgentChatPanes
            compact={layout.compact}
            navigationCollapsed={navigationCollapsed}
            onArtifactHeaderMount={setArtifactHeader}
          />
        </div>
        {!layout.artifactOverlay && artifactExpanded ? (
          <WorkbenchSidebarSplitter
            ariaLabel="Resize files and preview"
            side="trailing"
            value={layout.artifactWidth}
            width={layout.artifactBounds}
            onChange={onAgentArtifactWidthChange}
          />
        ) : null}
        {layout.artifactModal ? (
          <button
            className="agent-artifact-backdrop"
            type="button"
            tabIndex={-1}
            aria-label="Dismiss files and preview"
            onClick={() => onArtifactExpandedChange(false)}
          />
        ) : null}
        <AgentArtifactPane
          expanded={artifactExpanded}
          focusOnExpand={layout.artifactOverlay}
          modal={layout.artifactModal}
          overlayOnExpand={layout.artifactOverlayOnExpand}
          headerNode={artifactHeader}
          onExpandedChange={onArtifactExpandedChange}
          onOpenInEditor={onOpenArtifactInEditor}
        />
      </div>
    </AgentToolsProvider>
  );
}
