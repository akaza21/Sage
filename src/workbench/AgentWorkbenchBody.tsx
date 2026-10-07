import type { CSSProperties } from "react";

import { AgentArtifactPane } from "./AgentArtifactPane";
import { AgentChatPanes } from "./AgentChatPanes";
import { LeftSidebar } from "./LeftSidebar";
import { WorkbenchSidebarSplitter } from "./WorkbenchSidebarSplitter";
import type { WorkbenchSidebarWidth } from "./workbenchSidebarResize";

export function AgentWorkbenchBody({
  compact,
  style,
  drawerOpen,
  navigationOpen,
  onNavigationOpenChange,
  artifactExpanded,
  onArtifactExpandedChange,
  agentLeftRailWidth,
  agentLeftRailWidthBounds,
  onAgentLeftRailWidthChange,
  agentArtifactWidth,
  agentArtifactWidthBounds,
  onAgentArtifactWidthChange,
  onOpenArtifactInEditor,
  onOpenSettings,
}: {
  compact: boolean;
  style: CSSProperties | undefined;
  drawerOpen: boolean;
  navigationOpen: boolean;
  onNavigationOpenChange: (open: boolean) => void;
  artifactExpanded: boolean;
  onArtifactExpandedChange: (expanded: boolean) => void;
  agentLeftRailWidth: number;
  agentLeftRailWidthBounds: WorkbenchSidebarWidth;
  onAgentLeftRailWidthChange: (width: number) => void;
  agentArtifactWidth: number;
  agentArtifactWidthBounds: WorkbenchSidebarWidth;
  onAgentArtifactWidthChange: (width: number) => void;
  onOpenArtifactInEditor: (tabId: string) => void;
  onOpenSettings: () => void;
}) {
  return (
    <div className="workbench__body" style={style}>
      <LeftSidebar
        compact={compact}
        open={navigationOpen}
        onOpenChange={onNavigationOpenChange}
        onOpenSettings={onOpenSettings}
      />
      {!compact ? (
        <WorkbenchSidebarSplitter
          ariaLabel="Resize project and chat rail"
          side="leading"
          value={agentLeftRailWidth}
          width={agentLeftRailWidthBounds}
          onChange={onAgentLeftRailWidthChange}
        />
      ) : null}
      <div
        className="center-region"
        inert={drawerOpen}
        aria-hidden={drawerOpen || undefined}
      >
        <AgentChatPanes compact={compact} />
      </div>
      {!compact && artifactExpanded ? (
        <WorkbenchSidebarSplitter
          ariaLabel="Resize files and preview"
          side="trailing"
          value={agentArtifactWidth}
          width={agentArtifactWidthBounds}
          onChange={onAgentArtifactWidthChange}
        />
      ) : null}
      <AgentArtifactPane
        expanded={artifactExpanded}
        focusOnExpand={compact}
        onExpandedChange={onArtifactExpandedChange}
        onOpenInEditor={onOpenArtifactInEditor}
      />
    </div>
  );
}
