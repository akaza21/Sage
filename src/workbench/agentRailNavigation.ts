import { createContext } from "react";

export const agentRailSectionSelectors = {
  Recent: '[aria-label="Recents"]',
  Projects: '[aria-label="Projects"], [aria-label="Recents"]',
} as const;

export function navigateAgentHome() {
  const pane = document.querySelector<HTMLElement>(".agent-chat-pane--focused");
  const unavailable = '[hidden], [inert], [aria-hidden="true"]';
  if (!pane || pane.closest(unavailable)) return;
  const composer = pane.querySelector<HTMLTextAreaElement>(
    ".composer textarea:not(:disabled)",
  );
  if (composer && !composer.closest(unavailable)) {
    composer.focus();
    if (document.activeElement === composer) return;
  }
  pane.focus();
}
export const AgentHomeNavigationContext = createContext(navigateAgentHome);
