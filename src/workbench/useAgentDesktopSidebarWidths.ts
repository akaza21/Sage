import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  type AgentDesktopSidebarPriority,
  type AgentDesktopSidebarWidths,
} from "./workbenchSidebarResize";
import { agentWorkbenchLayout } from "./agentWorkbenchLayout";

type AgentSidebarOverrides = {
  left: boolean;
  artifact: boolean;
};

export function useAgentDesktopSidebarWidths(
  artifactExpanded: boolean,
  navigationCollapsed = false,
) {
  const initialLayout = agentWorkbenchLayout({
    viewportWidth: window.innerWidth,
    artifactExpanded: false,
  });
  const initialWidths = {
    leftRailWidth: initialLayout.leftRailWidth,
    artifactWidth: initialLayout.artifactWidth,
  };
  const requestedWidths = useRef<AgentDesktopSidebarWidths>(initialWidths);
  const overrides = useRef<AgentSidebarOverrides>({
    left: false,
    artifact: false,
  });
  const [viewportWidth, setViewportWidth] = useState(window.innerWidth);
  const [widthRequests, setWidthRequests] = useState(initialWidths);
  const [priority, setPriority] =
    useState<AgentDesktopSidebarPriority>("artifact");
  const requestsForViewport = useCallback(() => {
    const defaults = agentWorkbenchLayout({
      viewportWidth: window.innerWidth,
      artifactExpanded: false,
    });
    return {
      leftRailWidth: overrides.current.left
        ? requestedWidths.current.leftRailWidth
        : defaults.leftRailWidth,
      artifactWidth: overrides.current.artifact
        ? requestedWidths.current.artifactWidth
        : defaults.artifactWidth,
    };
  }, []);

  useEffect(() => {
    const resize = () => {
      setViewportWidth(window.innerWidth);
      setWidthRequests(requestsForViewport());
    };
    window.addEventListener("resize", resize);
    return () => window.removeEventListener("resize", resize);
  }, [requestsForViewport]);

  const changeWidth = useCallback(
    (prioritize: AgentDesktopSidebarPriority, width: number) => {
      setPriority(prioritize);
      overrides.current = {
        ...overrides.current,
        [prioritize]: true,
      };
      requestedWidths.current = {
        ...requestedWidths.current,
        ...(prioritize === "left"
          ? { leftRailWidth: width }
          : { artifactWidth: width }),
      };
      setWidthRequests(requestsForViewport());
    },
    [requestsForViewport],
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
