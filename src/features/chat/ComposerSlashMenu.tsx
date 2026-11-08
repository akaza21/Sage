import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useReducer,
  useState,
  type KeyboardEvent,
  type RefObject,
} from "react";
import { useStore } from "zustand";
import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import type {
  AcpChatPage,
  RuntimeEpoch,
  SageAgentSessionId,
} from "../../domain/acp";
import {
  acpCommandsDesktop,
  type AcpCommandList,
  type AvailableAcpCommand,
} from "../../services/acpCommandsDesktop";
import { AcpDesktopError } from "../../services/acpDesktop";
import {
  suggestionOptionId,
  useSuggestionNavigation,
  type TextFieldSuggestion,
  type TextFieldSuggestionsState,
} from "../../ui/textFieldSuggestionNavigation";
import { TextFieldSuggestions } from "../../ui/TextFieldSuggestions";
import { createClientIntentFactory } from "./acp/clientIntent";
import { currentContext } from "./acp/useAcpChatActions";
import type { PickedSlashCommand } from "./composerSlashCommand";

const nextIntentId = createClientIntentFactory();
const LOADING: Load = { kind: "loading" };

type Load =
  | { kind: "loading" }
  | { kind: "ready"; owner: string; list: AcpCommandList }
  | { kind: "error"; owner: string; message: string };

/** The current session when it has a live runtime that can list commands. */
function liveOwnerKey(page: AcpChatPage | null): string {
  const session = page?.sessions.find(
    (item) => item.id === page.currentSessionId,
  );
  return session?.runtimeEpoch &&
    session.residency === "hot" &&
    (session.state === "ready" || session.state === "active")
    ? JSON.stringify([session.id, session.runtimeEpoch])
    : "";
}

/** Prefix matches first, then other name matches, each in advertised order. */
function filterSlashCommands(
  commands: readonly AvailableAcpCommand[],
  query: string,
): AvailableAcpCommand[] {
  const needle = query.toLocaleLowerCase();
  const prefix: AvailableAcpCommand[] = [];
  const inner: AvailableAcpCommand[] = [];
  for (const command of commands) {
    const name = command.name.toLocaleLowerCase();
    if (name.startsWith(needle)) prefix.push(command);
    else if (name.includes(needle)) inner.push(command);
  }
  return [...prefix, ...inner];
}

function suggestion(command: AvailableAcpCommand): TextFieldSuggestion {
  return {
    id: command.name,
    label: `/${command.name}`,
    hint: command.inputHint ?? undefined,
    description: command.description || undefined,
    accessibleLabel: [
      `/${command.name}`,
      command.description,
      command.inputHint ? `Input: ${command.inputHint}` : "",
    ]
      .filter(Boolean)
      .join(", "),
  };
}

/** Reads the session's advertised commands and re-reads each new revision. */
function useSessionCommands(boundChatId: string | undefined): Load | null {
  const owner = useStore(acpProjectionStoreFor(boundChatId), (state) =>
    liveOwnerKey(state.page),
  );
  const [load, setLoad] = useState<Load>(LOADING);
  const [revision, changed] = useReducer((value: number) => value + 1, 0);
  // The owner whose change listener is installed. The list is read only
  // after it, so an update that lands during the read is not missed.
  const [listening, setListening] = useState("");
  useEffect(() => {
    if (!owner) return;
    const [sessionId, runtimeEpoch] = JSON.parse(owner) as [string, string];
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void acpCommandsDesktop
      .subscribeCommandChanges((event) => {
        if (
          event.sageAgentSessionId === sessionId &&
          event.runtimeEpoch === runtimeEpoch
        )
          changed();
      })
      .then(
        (dispose) => {
          if (disposed) return dispose();
          unlisten = dispose;
          setListening(owner);
        },
        // Without live updates the list is still read once.
        () => !disposed && setListening(owner),
      );
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [owner]);
  useEffect(() => {
    if (!owner || listening !== owner) return;
    const [sessionId, runtimeEpoch] = JSON.parse(owner) as [
      SageAgentSessionId,
      RuntimeEpoch,
    ];
    let current = true;
    void Promise.resolve()
      .then(() => {
        const context = currentContext(boundChatId);
        if (!context) throw new Error("This chat is not ready.");
        return acpCommandsDesktop.listCommands({
          ...context.scope,
          projectGeneration: context.commandProjectGeneration,
          expectedProjectionRevision: context.page.projectionRevision,
          clientIntentId: nextIntentId(),
          sessionId,
          runtimeEpoch,
        });
      })
      .then(
        (list) => {
          if (current) setLoad({ kind: "ready", owner, list });
        },
        (error: unknown) => {
          if (current)
            setLoad({
              kind: "error",
              owner,
              message:
                error instanceof AcpDesktopError || error instanceof Error
                  ? error.message
                  : "The command list could not be read.",
            });
        },
      );
    return () => {
      current = false;
    };
  }, [boundChatId, listening, owner, revision]);
  if (!owner) return null;
  // A live update keeps the last list until its re-read lands; another
  // session starts from loading.
  return load.kind === "loading" || load.owner === owner ? load : LOADING;
}

function menuState(
  load: Load | null,
  query: string,
  matches: number,
): TextFieldSuggestionsState {
  if (!load)
    return {
      kind: "empty",
      title: "Commands appear once the session is running.",
      detail: "Send a message to start it.",
    };
  if (load.kind === "loading")
    return { kind: "loading", title: "Loading commands…" };
  if (load.kind === "error")
    return {
      kind: "error",
      title: "Commands are unavailable.",
      detail: load.message,
    };
  if (load.list.commands.length === 0)
    return {
      kind: "empty",
      title: "This harness advertises no slash commands.",
    };
  if (matches === 0)
    return { kind: "empty", title: `No commands match “/${query}”.` };
  return { kind: "ready" };
}

export function ComposerSlashMenu({
  anchorRef,
  boundChatId,
  keyHandlerRef,
  listboxId,
  onActiveOptionChange,
  onDismiss,
  onPick,
  query,
}: {
  anchorRef: RefObject<HTMLElement | null>;
  boundChatId?: string;
  keyHandlerRef: RefObject<((event: KeyboardEvent) => boolean) | null>;
  listboxId: string;
  onActiveOptionChange: (optionId: string | null) => void;
  onDismiss: () => void;
  onPick: (command: PickedSlashCommand) => void;
  query: string;
}) {
  const load = useSessionCommands(boundChatId);
  const commands = useMemo(
    () => (load?.kind === "ready" ? load.list.commands : []),
    [load],
  );
  const items = useMemo(
    () => filterSlashCommands(commands, query).map(suggestion),
    [commands, query],
  );
  const pick = (item: TextFieldSuggestion) => {
    const command = commands.find((candidate) => candidate.name === item.id);
    if (command)
      onPick({ name: command.name, inputHint: command.inputHint ?? null });
  };
  const navigation = useSuggestionNavigation(items, pick, onDismiss);
  const activeId = navigation.active?.id ?? null;
  useLayoutEffect(() => {
    keyHandlerRef.current = navigation.onKeyDown;
  });
  useLayoutEffect(
    () => () => {
      keyHandlerRef.current = null;
    },
    [keyHandlerRef],
  );
  useLayoutEffect(() => {
    onActiveOptionChange(
      activeId ? suggestionOptionId(listboxId, activeId) : null,
    );
  }, [activeId, listboxId, onActiveOptionChange]);
  useLayoutEffect(
    () => () => onActiveOptionChange(null),
    [onActiveOptionChange],
  );
  const state = menuState(load, query, items.length);
  return (
    <TextFieldSuggestions
      anchorRef={anchorRef}
      ariaLabel="Slash commands"
      className="composer-slash-menu"
      items={items}
      activeId={activeId}
      listboxId={listboxId}
      onActivate={navigation.setActiveId}
      onDismiss={onDismiss}
      onPick={pick}
      state={state}
      status={
        state.kind === "ready"
          ? `${items.length} ${items.length === 1 ? "command" : "commands"}`
          : ""
      }
    />
  );
}
