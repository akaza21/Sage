import type { ChatMessage } from "../domain/workbench";

const MAX_BROWSER_MESSAGES = 500;
const MAX_BROWSER_MESSAGE_BYTES = 4 * 1024 * 1024;

function messageBytes(values: ChatMessage[]) {
  const encoder = new TextEncoder();
  return values.reduce(
    (total, message) => total + encoder.encode(message.text).byteLength,
    0,
  );
}

export function appendBrowserSimulatorTurn(
  current: ChatMessage[],
  request: {
    sessionId: string;
    modelId: string | null;
    text: string;
  },
  ids: [string, string] = [crypto.randomUUID(), crypto.randomUUID()],
  createdAt = Date.now(),
) {
  const sequence = (current.at(-1)?.sequence ?? 0) + 1;
  const response = `Simulator acknowledged the prompt using ${
    request.modelId ?? "the harness default"
  }. The ACP supervisor remains a later phase.`;
  const turn: ChatMessage[] = [
    {
      id: ids[0],
      sessionId: request.sessionId,
      sequence,
      role: "user",
      text: request.text,
      createdAt,
    },
    {
      id: ids[1],
      sessionId: request.sessionId,
      sequence: sequence + 1,
      role: "assistant",
      text: response,
      createdAt,
    },
  ];
  if (
    current.length + turn.length > MAX_BROWSER_MESSAGES ||
    messageBytes(current) + messageBytes(turn) > MAX_BROWSER_MESSAGE_BYTES
  ) {
    throw new Error(
      "browser session history capacity reached; existing messages were preserved",
    );
  }
  return [...current, ...turn];
}
