import type { RefObject } from "react";

export function AgentArtifactFocusGuard({
  active,
  paneRef,
}: {
  active: boolean;
  paneRef: RefObject<HTMLElement | null>;
}) {
  if (!active) return null;
  return (
    <span
      className="agent-artifact-pane__focus-guard"
      data-artifact-focus-guard
      tabIndex={0}
      aria-hidden="true"
      onFocus={() => paneRef.current?.focus()}
    />
  );
}
