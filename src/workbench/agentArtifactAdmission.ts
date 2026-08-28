export const MAX_AGENT_ADMITTED_ARTIFACTS = 32;

export function admitAgentArtifactIds(
  current: readonly string[],
  tabId: string,
): string[] {
  if (typeof tabId !== "string" || tabId.length === 0 || tabId.length > 512) {
    return [...current];
  }
  const next = current.filter((id) => id !== tabId);
  next.push(tabId);
  if (next.length > MAX_AGENT_ADMITTED_ARTIFACTS) {
    next.shift();
  }
  return next;
}

export function agentArtifactVisible(input: {
  tabId: string;
  kind: string;
  admittedIds: readonly string[];
  hasFocusedChatStats: boolean;
}): boolean {
  if (input.kind !== "file" && input.kind !== "diff") return false;
  return (
    agentArtifactAdmitted(input.admittedIds, input.tabId) ||
    input.hasFocusedChatStats
  );
}

export function agentArtifactAdmitted(
  current: readonly string[],
  tabId: string,
): boolean {
  return current.includes(tabId);
}
