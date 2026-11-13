import type {
  AcpMutationResult,
  AcpSessionMutationIntent,
  SendAcpTurnRequest,
} from "../../domain/acp";
import { AcpDesktopError } from "../../services/acpDesktop";
import { acpCommandsDesktop } from "../../services/acpCommandsDesktop";
import { pickedCommandFor } from "./slashCommandPicks";

export type SlashCommandPort = Pick<
  typeof acpCommandsDesktop,
  "listCommands" | "runCommand"
>;

/** `/name rest`: the harness reads the first token as a command. */
export function parseSlashCommand(
  prompt: string,
): { name: string; input: string } | null {
  const match = /^\/([^\s/]+)(?:\s+([\s\S]*))?$/u.exec(prompt);
  return match ? { name: match[1]!, input: match[2] ?? "" } : null;
}

/**
 * Sends a prompt naming a command the live session advertises through the
 * typed command route, which checks the list revision natively and marks
 * the turn as a command (Pi's extension commands depend on that). Returns
 * null for anything else, which goes out as plain prompt text. A command
 * the user picked from the menu that the session no longer offers is
 * refused instead of being sent as text.
 */
export async function slashCommandTurn(
  request: SendAcpTurnRequest,
  nextIntentId: () => AcpSessionMutationIntent["clientIntentId"],
  port?: SlashCommandPort,
): Promise<(() => Promise<AcpMutationResult>) | null> {
  if (request.target !== "session") return null;
  const { prompt, attachedContext } = request;
  const command = parseSlashCommand(prompt);
  // The typed route carries no attached context; such a prompt stays text.
  if (!command || attachedContext?.length) return null;
  const intent: AcpSessionMutationIntent = {
    projectId: request.projectId,
    projectGeneration: request.projectGeneration,
    chatId: request.chatId,
    expectedProjectionRevision: request.expectedProjectionRevision,
    clientIntentId: nextIntentId(),
    sessionId: request.sessionId,
    runtimeEpoch: request.runtimeEpoch,
  };
  const commands = port ?? acpCommandsDesktop;
  const list = await commands.listCommands(intent).catch(() => null);
  if (list?.commands.some((candidate) => candidate.name === command.name)) {
    return () =>
      commands.runCommand({
        ...intent,
        clientIntentId: nextIntentId(),
        expectedCommandsRevision: list.revision,
        name: command.name,
        ...(command.input ? { input: command.input } : {}),
      });
  }
  if (pickedCommandFor(request.chatId) === command.name) {
    throw new AcpDesktopError({
      kind: "invalidRequest",
      message: `This session no longer offers /${command.name}, so nothing was sent.`,
    });
  }
  return null;
}
