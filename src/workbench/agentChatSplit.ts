export const AGENT_CHAT_SPLIT_DEFAULT = 50;
export const AGENT_CHAT_SPLIT_MIN = 30;
export const AGENT_CHAT_SPLIT_MAX = 70;

export function resizeAgentChatSplit({
  containerWidth,
  pointerOffset,
}: {
  containerWidth: number;
  pointerOffset: number;
}): number {
  if (!Number.isFinite(containerWidth) || containerWidth <= 0) {
    return AGENT_CHAT_SPLIT_DEFAULT;
  }

  const percentage = Math.round((pointerOffset / containerWidth) * 100);
  return Math.min(
    AGENT_CHAT_SPLIT_MAX,
    Math.max(AGENT_CHAT_SPLIT_MIN, percentage),
  );
}
