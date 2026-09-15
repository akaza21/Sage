import type { CSSProperties } from "react";

import { AgentArtifactPane } from "./AgentArtifactPane";
import { AgentChatPanes } from "./AgentChatPanes";
import { LeftSidebar } from "./LeftSidebar";
import { WorkbenchSidebarSplitter } from "./WorkbenchSidebarSplitter";
import {
  AGENT_ARTIFACT_WIDTH,
  AGENT_LEFT_RAIL_WIDTH,
} from "./workbenchSidebarResize";

export function AgentWorkbenchBody({
  compact,
  style,
  drawerOpen,
  navigationOpen,
  onNavigationOpenChange,
  artifactExpanded,
  onArtifactExpandedChange,
  agentLeftRailWidth,
  onAgentLeftRailWidthChange,
  agentArtifactWidth,
  onAgentArtifactWidthChange,
  onOpenArtifactInEditor,
}: {
  compact: boolean;
  style: CSSProperties | undefined;
  drawerOpen: boolean;
  navigationOpen: boolean;
  onNavigationOpenChange: (open: boolean) => void;
  artifactExpanded: boolean;
  onArtifactExpandedChange: (expanded: boolean) => void;
  agentLeftRailWidth: number;
  onAgentLeftRailWidthChange: (width: number) => void;
  agentArtifactWidth: number;
  onAgentArtifactWidthChange: (width: number) => void;
  onOpenArtifactInEditor: (tabId: string) => void;
}) {
  return (
    <div className="workbench__body" style={style}>
      <LeftSidebar
        compact={compact}
        open={navigationOpen}
        onOpenChange={onNavigationOpenChange}
      />
      {!compact ? (
        <WorkbenchSidebarSplitter
          ariaLabel="Resize project and chat rail"
          side="leading"
          value={agentLeftRailWidth}
          width={AGENT_LEFT_RAIL_WIDTH}
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
          width={AGENT_ARTIFACT_WIDTH}
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
