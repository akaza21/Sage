/**
 * The question-answer command. Only the question card sends answers, so this
 * loads with that card rather than with the chat.
 */
import type {
  AnswerUserInputRequest,
  AnswerUserInputResponse,
} from "../domain/acpPermission";
import { isRecord } from "./acpDesktopGuards";
import { AcpDesktopError } from "./acpDesktopCommand";
import { safePermissionIdentity } from "./acpDesktopPermission";

function safeAnswerResponse(
  value: unknown,
  request: AnswerUserInputRequest,
): AnswerUserInputResponse | null {
  if (!isRecord(value)) return null;
  const identity = safePermissionIdentity(value, request);
  const matches =
    identity && value.decision === "answer" && value.state === "allowPending";
  return matches
    ? {
        ...identity,
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
): Promise<AnswerUserInputResponse> {
  const response = await invoke("answer_user_input", request);
  const safe = safeAnswerResponse(response, request);
  if (!safe)
    throw new AcpDesktopError({
      kind: "internal",
      message: "Question answer returned an invalid bounded response.",
    });
  return safe;
}
