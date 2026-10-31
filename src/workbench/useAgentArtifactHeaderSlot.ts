import { createContext, createElement, type ReactNode } from "react";

/** The active File contributes preview and details actions to its resource bar. */
export const AgentArtifactHeaderSlot = createContext<{
  actions: HTMLElement | null;
  details: HTMLElement | null;
} | null>(null);

export function AgentArtifactFileHeaderBridge({
  actions,
  details,
  children,
}: {
  actions: HTMLElement | null;
  details: HTMLElement | null;
  children: ReactNode;
}) {
  return createElement(
    AgentArtifactHeaderSlot.Provider,
    { value: { actions, details } },
    children,
  );
}
