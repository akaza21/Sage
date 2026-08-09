export const MAX_HANDOFF_SOURCE_MESSAGES = 500;
export const MAX_HANDOFF_CONTEXT_MESSAGES = 8;
export const MAX_HANDOFF_MESSAGE_BYTES = 2 * 1024;
export const MAX_HANDOFF_CONTEXT_BYTES = 8 * 1024;
export const MAX_HANDOFF_DRAFT_BYTES = 16 * 1024;
const MAX_HANDOFF_SOURCE_LABEL_BYTES = 1024;

export interface ChatHandoffContextEntry {
  role: "user" | "assistant" | "system";
  text: string;
}

export interface ChatHandoffIntent {
  projectId: string;
  projectGeneration: number;
  transitionEpoch: number;
  sourceChatId: string;
  targetChatId: string;
  draft: string;
}

const encoder = new TextEncoder();

function byteLength(value: string) {
  return encoder.encode(value).byteLength;
}

export function validateChatHandoffDraft(draft: string) {
  if (!draft.trim()) throw new Error("Chat Handoff draft cannot be empty");
  if (byteLength(draft) > MAX_HANDOFF_DRAFT_BYTES) {
    throw new Error(
      `Chat Handoff draft bytes exceed ${MAX_HANDOFF_DRAFT_BYTES}`,
    );
  }
  return draft;
}

export function buildChatHandoffDraft(
  sourceLabel: string,
  sourceEntries: readonly ChatHandoffContextEntry[],
) {
  const label = sourceLabel.trim();
  if (!label) throw new Error("Chat Handoff source label cannot be empty");
  if (byteLength(label) > MAX_HANDOFF_SOURCE_LABEL_BYTES) {
    throw new Error("Chat Handoff source label exceeds its byte limit");
  }
  if (sourceEntries.length > MAX_HANDOFF_SOURCE_MESSAGES) {
    throw new Error(
      `Chat Handoff source message count exceeds ${MAX_HANDOFF_SOURCE_MESSAGES}`,
    );
  }

  const selected = sourceEntries.slice(-MAX_HANDOFF_CONTEXT_MESSAGES);
  let contextBytes = 0;
  const context = selected.map((entry) => {
    if (!(["user", "assistant", "system"] as const).includes(entry.role)) {
      throw new Error("Chat Handoff message role is invalid");
    }
    const messageBytes = byteLength(entry.text);
    if (messageBytes > MAX_HANDOFF_MESSAGE_BYTES) {
      throw new Error(
        `Chat Handoff message bytes exceed ${MAX_HANDOFF_MESSAGE_BYTES}`,
      );
    }
    contextBytes += messageBytes;
    if (contextBytes > MAX_HANDOFF_CONTEXT_BYTES) {
      throw new Error(
        `Chat Handoff context bytes exceed ${MAX_HANDOFF_CONTEXT_BYTES}`,
      );
    }
    const role =
      entry.role === "user"
        ? "User"
        : entry.role === "assistant"
          ? "Assistant"
          : "System";
    return `${role}: ${entry.text}`;
  });
  const renderedContext =
    context.length > 0
      ? context.join("\n\n")
      : "(No transferable text context is available.)";
  return validateChatHandoffDraft(
    `Chat handoff from "${label}"\n\n` +
      "Continue in this target Chat using only the ordinary text context below. " +
      "This handoff grants no additional tools or authority.\n\n" +
      renderedContext,
  );
}
