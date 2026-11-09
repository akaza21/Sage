import type { ComponentProps, ComponentType } from "react";
import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import type { AvailableAcpCommand } from "../../services/acpCommandsDesktop";
import { sessionActionAvailable } from "./acp/sessionAction";
import { chatActionController } from "./acp/useAcpChatActions";
import { lazyParts } from "./lazyParts";
import type * as PanelChunk from "./ComposerCommandPanel";

// The review target picker and the status panel load with their command.
const PanelPart = lazyParts<typeof PanelChunk>(
  () => import("./ComposerCommandPanel"),
);
type PanelProps = Omit<
  ComponentProps<typeof PanelChunk.ComposerCommandPanel>,
  "kind"
>;
const panel = (kind: "review" | "status") => (props: PanelProps) => (
  <PanelPart part="ComposerCommandPanel" kind={kind} {...props} />
);

/**
 * One row of the composer's `/` menu. A harness command writes `/name ` into
 * the field. The chat's own commands write their `text` in place of the typed
 * name (a prompt command), then run a session action or open a panel.
 */
export interface ComposerCommand extends AvailableAcpCommand {
  /** Unlike the name, so a fully typed name picks the command. */
  id?: string;
  text?: string;
  /** The panel the command opens. */
  panel?: ComponentType<PanelProps>;
  run?: () => void;
}

const own = (
  name: string,
  description: string,
  rest: Omit<ComposerCommand, "name" | "description" | "inputHint"> = {},
): ComposerCommand => ({
  id: `:${name}`,
  name,
  description,
  inputHint: null,
  text: "",
  ...rest,
});

/**
 * The live session's commands: the actions it negotiated, then the
 * harness's own commands, then the chat's own. A negotiated action takes
 * its name from a harness command; a harness command takes its name from
 * the chat's own. Loads with the first open menu.
 */
export function composerCommands(
  advertised: readonly AvailableAcpCommand[],
  boundChatId?: string,
): ComposerCommand[] {
  const { page } = acpProjectionStoreFor(boundChatId).getState();
  const actions = [
    sessionActionAvailable(page, "review") &&
      own(
        "review",
        "Review changes, a branch, a commit or anything you describe",
        {
          panel: panel("review"),
        },
      ),
    sessionActionAvailable(page, "compact") &&
      own("compact", "Summarize earlier turns to free context", {
        run: () =>
          void chatActionController(boundChatId).runSessionAction({
            kind: "compact",
          }),
      }),
  ].filter((command) => command !== false);
  const listed = [
    ...actions,
    ...advertised.filter(
      (command) => !actions.some(({ name }) => name === command.name),
    ),
  ];
  return [
    ...listed,
    ...[
      own("init", "Ask the agent to write an AGENTS.md for this project", {
        text: "Create an AGENTS.md file at the root of this project that tells coding agents how the project is laid out, how to build and test it, and which conventions to follow.",
      }),
      own("status", "Show the model, effort, access and usage", {
        panel: panel("status"),
      }),
    ].filter((command) => !listed.some(({ name }) => name === command.name)),
  ];
}
