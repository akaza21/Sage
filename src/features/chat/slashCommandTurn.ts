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

export function parseSlashCommand(
  prompt: string,
): { name: string; input: string } | null {
  const match = /^\/([^\s/]+)(?:\s+([\s\S]*))?$/u.exec(prompt);
  return match ? { name: match[1]!, input: match[2] ?? "" } : null;
}

// The typed command route marks the turn as a command; Pi's extension commands
// depend on that. `false` means the composer took the command and sends nothing.
export async function slashCommandTurn(
  request: SendAcpTurnRequest,
  nextIntentId: () => AcpSessionMutationIntent["clientIntentId"],
  port?: SlashCommandPort,
): Promise<(() => Promise<AcpMutationResult>) | null | false> {
  if (request.target !== "session") return null;
  const { prompt, attachedContext } = request;
  const command = parseSlashCommand(prompt);
  if (!command) return null;
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
  const reported = list?.commands.some(
    (candidate) => candidate.name === command.name,
  );
  // The typed route carries no attached context; such a prompt stays text.
  if (list && reported && !attachedContext?.length) {
    return () =>
      commands.runCommand({
        ...intent,
        clientIntentId: nextIntentId(),
        expectedCommandsRevision: list.revision,
        name: command.name,
        ...(command.input ? { input: command.input } : {}),
      });
  }
  // A curated terminal-only command waits for the user's choice.
  if (list?.terminalHandoff && !reported) {
    const route = await import("./ComposerTerminalCommand");
    if (route.routeTerminalCommand(request.chatId, prompt, command.name, list))
      return false;
  }
  if (pickedCommandFor(request.chatId) === command.name) {
    throw new AcpDesktopError({
      kind: "invalidRequest",
      message: `This session no longer offers /${command.name}, so nothing was sent.`,
    });
  }
  return null;
}
