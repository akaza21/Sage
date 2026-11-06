import { useCallback, useEffect, useMemo, useState } from "react";

import {
  type AgentDesktopSidebarPriority,
  type AgentDesktopSidebarWidths,
} from "./workbenchSidebarResize";
import { agentWorkbenchLayout } from "./agentWorkbenchLayout";

export function useAgentDesktopSidebarWidths(
  artifactExpanded: boolean,
  navigationCollapsed = false,
) {
  const [viewportWidth, setViewportWidth] = useState(window.innerWidth);
  // Only widths the person dragged to; the layout derives the rest from the
  // window, so untouched sidebars keep following resizes.
  const [widthRequests, setWidthRequests] = useState<
    Partial<AgentDesktopSidebarWidths>
  >({});
  const [priority, setPriority] =
    useState<AgentDesktopSidebarPriority>("artifact");

  useEffect(() => {
    const resize = () => setViewportWidth(window.innerWidth);
    window.addEventListener("resize", resize);
    return () => window.removeEventListener("resize", resize);
  }, []);

  const changeWidth = useCallback(
    (prioritize: AgentDesktopSidebarPriority, width: number) => {
      setPriority(prioritize);
      setWidthRequests((requests) => ({
        ...requests,
        [prioritize === "left" ? "leftRailWidth" : "artifactWidth"]: width,
      }));
    },
    [],
  );

  const layout = useMemo(
    () =>
      agentWorkbenchLayout({
        viewportWidth,
        artifactExpanded,
        navigationCollapsed,
        requestedWidths: widthRequests,
        prioritize: priority,
      }),
    [
      artifactExpanded,
      navigationCollapsed,
      priority,
      viewportWidth,
      widthRequests,
    ],
  );

  return { changeWidth, layout };
}
