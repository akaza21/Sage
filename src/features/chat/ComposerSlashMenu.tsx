import { useEffect, useLayoutEffect, useMemo, type RefObject } from "react";
import {
  type AcpCommandSource,
  type AvailableAcpCommand,
} from "../../services/acpCommandsDesktop";
import {
  suggestionOptionId,
  TEXT_FIELD_SUGGESTIONS_MAX_RENDERED,
  useSuggestionNavigation,
  type SuggestionKeyEvent,
  type TextFieldSuggestion,
  type TextFieldSuggestionsState,
} from "../../ui/textFieldSuggestionNavigation";
import { isImeComposing } from "../../ui/imeComposition";
import type { CommandLoad as Load } from "./ComposerCommandRefresh";
import { lazyParts } from "./lazyParts";
import { SuggestionsPart } from "./composerSuggestionsPart";
import { SlashCommandIcon } from "./SlashCommandIcon";
import type { PickedSlashCommand } from "./composerSlashCommand";

/**
 * Rows each section shows before typing, so every source stays in view:
 * seven sources of seven rows fit the list's 50 rendered rows.
 */
const SECTION_ROWS = 7;

const commandSourceGroups = {
  builtin: "Commands",
  skill: "Skills",
  prompt: "Prompts",
  extension: "Extensions",
  mcp: "MCP prompts",
  plugin: "Plugins",
  harness: "Other commands",
} as const satisfies Record<AcpCommandSource, string>;

function filterSlashCommands(
  commands: readonly AvailableAcpCommand[],
  query: string,
): AvailableAcpCommand[] {
  if (!query)
    return (Object.keys(commandSourceGroups) as AcpCommandSource[]).flatMap(
      (source) => {
        const rows = commands
          .filter((command) => command.source === source)
          .slice(0, SECTION_ROWS);
        const labels = new Map<string, AvailableAcpCommand[]>();
        for (const row of rows) {
          const label = row.sourceLabel ?? "";
          const group = labels.get(label) ?? [];
          group.push(row);
          labels.set(label, group);
        }
        return [...labels.values()].flat();
      },
    );
  const needle = query.toLocaleLowerCase();
  const ranks: AvailableAcpCommand[][] = [[], [], [], []];
  for (const command of commands) {
    const names = [command.name, ...(command.aliases ?? [])].map((name) =>
      name.toLocaleLowerCase(),
    );
    const rank =
      names[0] === needle
        ? 0
        : names.includes(needle)
          ? 1
          : names.some((name) => name.startsWith(needle))
            ? 2
            : names.some((name) => name.includes(needle))
              ? 3
              : -1;
    ranks[rank]?.push(command);
  }
  return ranks.flat();
}

function suggestion(
  command: AvailableAcpCommand,
  grouped: boolean,
): TextFieldSuggestion {
  return {
    id: command.name,
    group: grouped
      ? [commandSourceGroups[command.source], command.sourceLabel]
          .filter(Boolean)
          .join(" · ")
      : undefined,
    label: `/${command.name}`,
    icon: <SlashCommandIcon name={command.name} />,
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
  // Revision 0: the session has not sent its first list yet.
  if (load.list.revision === 0)
    return {
      kind: "loading",
      title: "Waiting for the session's commands…",
      detail: "The harness has not advertised any yet.",
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

const RefreshPart = lazyParts<typeof import("./ComposerCommandRefresh")>(
  () => import("./ComposerCommandRefresh"),
);

function MenuContent({
  load,
  anchorRef,
  boundChatId,
  keyHandlerRef,
  listboxId,
  onActiveOptionChange,
  onDismiss,
  onMountedChange,
  onPick,
  onStatusChange,
  query,
}: {
  load: Load | null;
  anchorRef: RefObject<HTMLElement | null>;
  boundChatId?: string;
  keyHandlerRef: RefObject<((event: SuggestionKeyEvent) => boolean) | null>;
  listboxId: string;
  onActiveOptionChange: (optionId: string | null) => void;
  onDismiss: () => void;
  onMountedChange: (mounted: boolean) => void;
  onPick: (command: PickedSlashCommand) => void;
  onStatusChange: (status: string) => void;
  query: string;
}) {
  const commands = useMemo(
    () => (load?.kind === "ready" ? load.list.commands : []),
    [load],
  );
  const items = useMemo(
    () =>
      filterSlashCommands(commands, query).map((command) =>
        suggestion(command, !query),
      ),
    [commands, query],
  );
  const pick = (item: TextFieldSuggestion) => {
    const command = commands.find((candidate) => candidate.name === item.id);
    if (command)
      onPick({
        name: command.name,
        inputHint: command.inputHint ?? null,
        ...(command.panel ? { panel: command.panel } : {}),
      });
  };
  const navigation = useSuggestionNavigation(items, pick, onDismiss);
  const more =
    (query ? items.length : commands.length) -
    Math.min(items.length, TEXT_FIELD_SUGGESTIONS_MAX_RENDERED);
  const omitted = load?.kind === "ready" ? (load.list.omitted ?? 0) : 0;
  const activeId = navigation.active?.id ?? null;
  const loading = load?.kind === "loading";
  useLayoutEffect(() => {
    keyHandlerRef.current = (event) => {
      const enter =
        event.key === "Enter" &&
        !event.shiftKey &&
        !event.altKey &&
        !event.ctrlKey &&
        !event.metaKey &&
        !isImeComposing(event);
      // As in the Claude Code CLI, a fully typed name runs on the first Enter.
      if (enter && navigation.active?.id === query) {
        if (!commands.find((command) => command.name === query)?.panel)
          return false;
        event.preventDefault();
        pick(navigation.active);
        return true;
      }
      // While suggestions load, Enter must not send a half-typed command.
      if (enter && loading) {
        event.preventDefault();
        return true;
      }
      return navigation.onKeyDown(event);
    };
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
  useLayoutEffect(() => {
    onMountedChange(true);
    return () => onMountedChange(false);
  }, [onMountedChange]);
  const state = menuState(load, query, items.length);
  const status =
    state.kind === "ready"
      ? `${items.length} ${items.length === 1 ? "command" : "commands"}`
      : state.title;
  useEffect(() => onStatusChange(status), [onStatusChange, status]);
  return (
    <SuggestionsPart
      part="TextFieldSuggestions"
      anchorRef={anchorRef}
      footer={
        more > 0 || omitted > 0 || load?.kind === "ready" ? (
          <RefreshPart
            part="ComposerCommandRefresh"
            more={more}
            omitted={omitted}
            boundChatId={boundChatId}
            owner={load?.kind === "ready" ? load.owner : null}
            pending={load?.kind === "ready" && load.list.pending === true}
            onStatusChange={onStatusChange}
          />
        ) : undefined
      }
      ariaLabel="Slash commands"
      className="composer-slash-menu"
      items={items}
      activeId={activeId}
      listboxId={listboxId}
      onActivate={navigation.setActiveId}
      onDismiss={onDismiss}
      onPick={pick}
      state={state}
    />
  );
}

export function ComposerSlashMenu(
  props: Omit<import("react").ComponentProps<typeof MenuContent>, "load">,
) {
  return (
    <RefreshPart part="ComposerCommandCatalog" boundChatId={props.boundChatId}>
      {(load) => <MenuContent {...props} load={load} />}
    </RefreshPart>
  );
}
