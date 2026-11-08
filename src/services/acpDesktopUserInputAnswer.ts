/**
 * The question-answer command. Only the question card sends answers, so this
 * loads with that card rather than with the chat.
 */
import type {
  AnswerUserInputRequest,
  AnswerUserInputResponse,
} from "../domain/acpPermission";
import { isRecord, safeCounter, safeIdentifier } from "./acpDesktopGuards";

function safeAnswerResponse(
  value: unknown,
  request: AnswerUserInputRequest,
): AnswerUserInputResponse | null {
  if (!isRecord(value)) return null;
  const runtimeEpoch = safeCounter(value.runtimeEpoch, false);
  const matches =
    safeIdentifier(value.projectId) === request.projectId &&
    safeIdentifier(value.chatId) === request.chatId &&
    safeIdentifier(value.sessionId) === request.sessionId &&
    safeIdentifier(value.turnId) === request.turnId &&
    runtimeEpoch === request.runtimeEpoch &&
    safeIdentifier(value.requestId) === request.requestId &&
    value.decision === "answer" &&
    value.state === "allowPending";
  return matches
    ? {
        projectId: request.projectId,
        chatId: request.chatId,
        sessionId: request.sessionId,
        turnId: request.turnId,
        runtimeEpoch: request.runtimeEpoch,
        requestId: request.requestId,
        decision: "answer",
        state: "allowPending",
      }
    : null;
}

/** Answers a harness question request; the reply never echoes answers. */
export async function answerUserInput(
  request: AnswerUserInputRequest,
  invoke: (
    command: string,
    request: AnswerUserInputRequest,
  ) => Promise<unknown>,
  invalidResponse: () => Error,
): Promise<AnswerUserInputResponse> {
  const response = await invoke("answer_user_input", request);
  const safe = safeAnswerResponse(response, request);
  if (!safe) throw invalidResponse();
  return safe;
}
