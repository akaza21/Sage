import type { ComponentProps } from "react";
import { lazyParts } from "./lazyParts";
import type * as Runtime from "./ComposerCommandRuntime";
import { useWorkbenchStore, type WorkbenchState } from "../../app/store";
import { invokeNative } from "../../ports/nativeInvoke";
import { AcpDesktopError } from "../../services/acpDesktop";
const loadRuntime = () => import("./ComposerCommandRuntime");
const RuntimePart =
  lazyParts<Pick<typeof Runtime, "ComposerSlashController">>(loadRuntime);

/** How long a new chat's first send naming a built-in command waits for the
 * session's command catalog before refusing to send. */
export const FIRST_COMMAND_HOLD_MS = 5_000;

// New chats whose first send names a curated built-in command, one entry per
// chat while that send is in flight.
const holds = new Map<string, { name: string; live: () => boolean }>();

const notSent = (reason: string) =>
  new AcpDesktopError({
    kind: "invalidRequest",
    message: `${reason} Nothing was sent.`,
  });

// Both field mounting and typed sends enter through this lightweight facade.
// A held first send waits here until the new session's harness reports its
// catalog (revision 0 until then), Stop ends the hold, or it times out; it
// then runs as a command or sends nothing, never text.
// eslint-disable-next-line react-refresh/only-export-components -- the lazy facade exposes the chunk's command action
export async function slashCommandTurn(
  ...args: Parameters<typeof Runtime.slashCommandTurn>
) {
  const runtime = await loadRuntime();
  const [request, nextIntentId, port] = args;
  const hold = holds.get(request.chatId);
  holds.delete(request.chatId);
  if (
    request.target !== "session" ||
    !hold ||
    hold.name !== /^\/([^\s/]+)/u.exec(request.prompt)?.[1] ||
    request.attachedContext?.length
  )
    return runtime.slashCommandTurn(...args);
  const intent = {
    projectId: request.projectId,
    projectGeneration: request.projectGeneration,
    chatId: request.chatId,
    expectedProjectionRevision: request.expectedProjectionRevision,
    clientIntentId: nextIntentId(),
    sessionId: request.sessionId,
    runtimeEpoch: request.runtimeEpoch,
  };
  for (let polls = FIRST_COMMAND_HOLD_MS / 200; ; polls--) {
    if (!hold.live()) throw notSent(`Stopped before /${hold.name} ran.`);
    const list = await (port ?? runtime.acpCommandsDesktop)
      .listCommands(intent)
      .catch(() => null);
    if (list?.revision) {
      if (list.commands.some(({ name }) => name === hold.name)) break;
      throw notSent(`/${hold.name} is not available in this session.`);
    }
    if (!polls)
      throw notSent(
        `/${hold.name} did not run: the session's commands took over ${FIRST_COMMAND_HOLD_MS / 1000} seconds.`,
      );
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  // As a picked command, it is refused rather than sent as text.
  runtime.setPickedCommand(request.chatId, hold.name);
  try {
    return await runtime.slashCommandTurn(...args);
  } finally {
    runtime.setPickedCommand(request.chatId, null);
  }
}

/**
 * A new chat's first send. One starting with `/` stays pending, held (the
 * composer shows it working, with Stop), while Sage looks up the harness's
 * curated built-in commands (installation metadata only). One of those waits
 * for the session's catalog in slashCommandTurn; anything else is sent now.
 */
// eslint-disable-next-line react-refresh/only-export-components -- the first send's command path
export async function sendFirstCommand(
  chatId: string,
  harnessId: string,
  pending: Extract<WorkbenchState["pendingFirstSend"], { text: string }>,
  submit: () => Promise<unknown>,
) {
  const store = useWorkbenchStore;
  const name = /^\/([^\s/]+)/u.exec(pending.text.trim())?.[1];
  if (!name) return submit();
  const held = { ...pending, held: true as const };
  const live = () => store.getState().pendingFirstSend === held;
  // Navigating away meanwhile cancelled it.
  if (
    store.getState().pendingFirstSend ||
    store.getState().activeTabId !== chatId
  )
    return;
  store.setState({ pendingFirstSend: held });
  const builtins = await invokeNative<unknown>("list_builtin_commands", {
    harnessId,
  }).catch(() => null);
  try {
    // Stopped while looking: nothing is sent.
    if (!live()) return;
    if (name && Array.isArray(builtins) && builtins.includes(name))
      holds.set(chatId, { name, live });
    else store.setState({ pendingFirstSend: null });
    await submit();
  } finally {
    holds.delete(chatId);
    if (live()) store.setState({ pendingFirstSend: null });
  }
}

export function ComposerSlashController(
  props: ComponentProps<typeof Runtime.ComposerSlashController>,
) {
  return <RuntimePart part="ComposerSlashController" {...props} />;
}
