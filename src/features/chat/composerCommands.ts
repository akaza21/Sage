import type { AcpChatPage, AcpSessionAction } from "../../domain/acp";
import type { AvailableAcpCommand } from "../../services/acpCommandsDesktop";
import { sessionActionAvailable } from "./acp/sessionAction";

/**
 * One row of the composer's `/` menu. A prompt command writes text into the
 * field: the harness's own commands write `/name `, the chat's own write
 * their `text`. An action command runs a session action the session
 * negotiated. A panel command opens a read-only panel.
 */
export type ComposerCommand = {
  name: string;
  description: string;
  inputHint?: string | null;
} & (
  | { kind: "prompt"; text?: string }
  | { kind: "action"; action: AcpSessionAction["kind"] }
  | { kind: "panel" }
);

const ACTIONS: readonly ComposerCommand[] = [
  {
    kind: "action",
    name: "review",
    action: "review",
    description: "Review changes, a branch, a commit or anything you describe",
  },
  {
    kind: "action",
    name: "compact",
    action: "compact",
    description: "Summarize earlier turns to free context",
  },
];

const OWN: readonly ComposerCommand[] = [
  {
    kind: "prompt",
    name: "init",
    description: "Ask the agent to write an AGENTS.md for this project",
    text: "Create an AGENTS.md file at the root of this project that tells coding agents how the project is laid out, how to build and test it, and which conventions to follow.",
  },
  {
    kind: "panel",
    name: "status",
    description: "Show the model, effort, access and usage",
  },
];

/**
 * The live session's commands: the actions it negotiated, then the
 * harness's own commands, then the chat's own. A negotiated action takes
 * its name from a harness command; a harness command takes its name from
 * the chat's own.
 */
export function composerCommands(
  page: AcpChatPage | null,
  advertised: readonly AvailableAcpCommand[],
): ComposerCommand[] {
  const actions = ACTIONS.filter(
    (command) =>
      command.kind === "action" && sessionActionAvailable(page, command.action),
  );
  const listed = [
    ...actions,
    ...advertised
      .filter((command) => !actions.some(({ name }) => name === command.name))
      .map((command) => ({ ...command, kind: "prompt" as const })),
  ];
  return [
    ...listed,
    ...OWN.filter((own) => !listed.some(({ name }) => name === own.name)),
  ];
}
