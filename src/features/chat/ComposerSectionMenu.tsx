import { ComposerMenuLoading } from "./ComposerMenuLoading";
import type * as CommandRuntime from "./ComposerCommandRuntime";
import { useContext, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useStore } from "zustand";
import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import { AgentToolsContext } from "../../workbench/AgentToolsProvider";
import type {
  ComposerAddActionsProps,
  ComposerOptionsProps,
} from "./ComposerAddActions";
import { composerNativeRows } from "./composerNativeRows";
import { composerMenuSections } from "./composerMenuSections";
import { ComposerMenuSections } from "./ComposerSectionList";
import type { SessionCommandsLoad } from "./useSessionCommands";
import "../../styles/composer-menu.css";
import { MenuPopover } from "../../ui/MenuPopover";
import { Notice } from "../../ui/Notice";
import {
  sessionConfigControls,
  sessionModeActions,
  flattenChoices,
} from "../../domain/acpSessionConfigControls";
import { withoutGovernedModes } from "../../domain/governedSessionModes";
import { usePinnedAgentChatOwner } from "../../app/agentChatOwner";
import type { AvailableAcpCommand } from "../../services/acpCommandsDesktop";
import { lazyParts } from "./lazyParts";
import { useWorkbenchStore } from "../../app/store";
import { useDraftBuiltinCommands } from "./draftBuiltinCommands";
import type * as PanelChunk from "./ComposerCommandPanel";
import type * as AddActionsChunk from "./ComposerAddActions";
import type { pickComposerSlashCommand } from "./insertSlashCommand";
import type { AcpCommandPanel } from "../../domain/acpCommand";
// A new-chat draft's image intake and handoff upload ride in this chunk:
// "Add images" lives here, and a draft's drops and sends load it too.
// eslint-disable-next-line react-refresh/only-export-components -- the draft image actions, shared by this lazy chunk's importers
export { addDraftImages } from "./draftImages";
const PanelPart = lazyParts<typeof PanelChunk>(
  () => import("./ComposerCommandPanel"),
);
const ExtensionsPart = lazyParts<
  Pick<typeof AddActionsChunk, "ExtensionsPopover">
>(() => import("./ComposerAddActions"));

function ComposerOptionsBrowser({
  load,
  insertCommand,
  id,
  triggerRef,
  onDismiss,
  selectionKey,
  canSelectEffort,
  focused,
  onModeRemoved,
  accessModes,
  ...given
}: ComposerOptionsProps &
  DraftBuiltinsProp & {
    load: SessionCommandsLoad | null;
    insertCommand?: typeof pickComposerSlashCommand;
  }) {
  const pinnedOwner = usePinnedAgentChatOwner();
  const owner = given.boundChatId ?? pinnedOwner;
  const [panel, setPanel] = useState<
    AcpCommandPanel | "managedMcp" | "extensions" | null
  >(null);
  const fieldRef = useMemo(
    () => ({
      get current() {
        return (
          triggerRef.current?.form?.querySelector<HTMLTextAreaElement>(
            "textarea",
          ) ?? null
        );
      },
    }),
    [triggerRef],
  );
  const pickCommand = (command: AvailableAcpCommand, input: string) => {
    if (given.disabled) return;
    if (command.panel) {
      setPanel(command.panel);
      return;
    }
    const field = fieldRef.current;
    if (!field || field.disabled) return;
    given.onClose();
    insertCommand?.(field, command, input);
  };
  const actions = {
    ...given,
    sessionConfig: withoutGovernedModes(given.sessionConfig, accessModes),
  };
  const actionsKey = JSON.stringify([
    owner,
    selectionKey,
    actions.sessionConfig?.sageAgentSessionId,
    !!actions.onAttachFiles,
    !!actions.planAccess,
    !!actions.onOpenGoal,
    !!actions.onClearGoal,
    actions.goalUnobserved,
    !!actions.onCompact,
    canSelectEffort,
    sessionModeActions(actions.sessionConfig).map((mode) => mode.id),
    sessionConfigControls(actions.sessionConfig, "other").map((option) => [
      option.id,
      option.type,
      flattenChoices(option).map((choice) => choice.value),
    ]),
  ]);
  const previousActionsKey = useRef(actionsKey);
  useLayoutEffect(() => {
    if (previousActionsKey.current !== actionsKey) {
      const element = document.activeElement;
      const menuHadFocus =
        element === document.body ||
        document.getElementById(id)?.contains(element);
      onDismiss();
      if (focused && menuHadFocus) onModeRemoved();
    }
    previousActionsKey.current = actionsKey;
  }, [actionsKey, focused, id, onDismiss, onModeRemoved]);
  if (panel === "extensions" || panel === "managedMcp")
    return (
      <ExtensionsPart
        part="ExtensionsPopover"
        kind={panel}
        load={load}
        boundChatId={owner ?? undefined}
        triggerRef={fieldRef}
        onClose={onDismiss}
        onRunCommand={(command) => pickCommand(command, "")}
      />
    );
  if (panel)
    return (
      <PanelPart
        part="ComposerCommandPanel"
        kind={panel}
        boundChatId={owner ?? undefined}
        fieldRef={fieldRef}
        onClose={onDismiss}
      />
    );
  return (
    <MenuPopover
      id={id}
      className="composer__popover composer__options"
      ariaLabel="Composer options menu"
      initialFocus="selected"
      focusKey="options"
      restoreFocusOnTab
      open
      triggerRef={triggerRef}
      onDismiss={onDismiss}
      gap={8}
    >
      <ComposerSectionRows
        {...actions}
        load={load}
        onOpenUsage={() => setPanel("status")}
        onOpenMcp={() =>
          setPanel(
            load?.kind === "ready" &&
              load.list.commands.some((command) => command.panel === "mcp")
              ? "mcp"
              : "managedMcp",
          )
        }
        onOpenExtensions={() => setPanel("extensions")}
        onPickCommand={pickCommand}
      />
    </MenuPopover>
  );
}

interface DraftBuiltinsProp {
  /** The new-chat draft's curated built-in commands, insert-only. */
  draftBuiltins?: readonly AvailableAcpCommand[];
}

export function ComposerSectionRows({
  load = null,
  draftBuiltins,
  ...props
}: ComposerAddActionsProps &
  DraftBuiltinsProp & { load?: SessionCommandsLoad | null }) {
  const pinnedOwner = usePinnedAgentChatOwner();
  const owner = props.boundChatId ?? pinnedOwner;
  const page = useStore(acpProjectionStoreFor(owner), (state) => state.page);
  const [query, setQuery] = useState("");
  const searchRef = useRef<HTMLInputElement>(null);
  const { launch, chatAvailable } = useContext(AgentToolsContext);
  const importable = !!page?.sessions.some(
    (session) =>
      session.id === page.currentSessionId &&
      session.capabilities.nativeSessionImport,
  );
  const rows = composerNativeRows({
    ...props,
    importable: chatAvailable && importable,
    onImport: () => launch("sessionImport", owner ?? undefined),
    onOpenUsage: page?.usage ? props.onOpenUsage : undefined,
    onOpenMcp:
      load?.kind === "ready" &&
      // A Sage MCP panel, or the harness's own MCP command placed on the row.
      load.list.commands.some(
        (command) =>
          command.panel === "mcp" || command.placement?.control === "mcpPanel",
      )
        ? props.onOpenMcp
        : undefined,
    // Built from reported provenance only, so a harness reporting none has no row.
    onOpenExtensions:
      load?.kind === "ready" &&
      load.list.commands.some((command) => command.sourceInfo)
        ? props.onOpenExtensions
        : undefined,
  });
  const sections = composerMenuSections(
    rows,
    load?.kind === "ready" ? load.list.commands : [],
    query,
    props.draftPickers ? (draftBuiltins ?? []) : undefined,
  );
  return (
    <div
      onKeyDown={(event) => {
        if (event.target instanceof HTMLInputElement) return;
        if (
          event.key === " " &&
          event.target instanceof Element &&
          event.target.closest("button, [role^='menuitem']")
        ) {
          event.stopPropagation();
          return;
        }
        if (
          event.key.length === 1 &&
          !event.ctrlKey &&
          !event.metaKey &&
          !event.altKey &&
          !event.nativeEvent.isComposing
        ) {
          event.preventDefault();
          event.stopPropagation();
          setQuery((value) => (value + event.key).slice(0, 256));
          searchRef.current?.focus();
        }
      }}
    >
      <input
        ref={searchRef}
        className="composer-menu__search"
        type="search"
        aria-label="Search composer options"
        placeholder="Search options and commands"
        maxLength={256}
        value={query}
        onChange={(event) => setQuery(event.target.value.slice(0, 256))}
        onKeyDown={(event) => {
          if (
            event.key !== "Escape" &&
            event.key !== "Tab" &&
            event.key !== "ArrowDown" &&
            event.key !== "ArrowUp"
          )
            event.stopPropagation();
        }}
      />
      <ComposerMenuSections
        sections={sections}
        disabled={props.disabled}
        onPick={(command, input) => props.onPickCommand?.(command, input)}
      />
      {query && !sections.length ? (
        <Notice
          identity="composer-options-empty"
          severity="info"
          progress
          message="No matching options or commands."
        />
      ) : null}
      {load?.kind === "loading" ? (
        <Notice
          identity="composer-commands-loading"
          severity="info"
          progress
          message="Loading commands..."
        />
      ) : null}
      {load?.kind === "error" ? (
        <Notice
          identity="composer-commands-unavailable"
          severity="warning"
          announce={false}
          message={`Commands are unavailable. ${load.message}`}
        />
      ) : null}
      {load?.kind === "ready" && !!load.list.omitted ? (
        <Notice
          identity="composer-commands-truncated"
          severity="info"
          message={`Catalog truncated: ${load.list.omitted} commands not shown`}
        />
      ) : null}
    </div>
  );
}

/** The draft lists its harness's curated built-ins; a pick only inserts
 * `/name ` into the draft's text. */
function DraftOptionsBrowser(props: ComposerOptionsProps) {
  const builtins = useDraftBuiltinCommands(
    useWorkbenchStore((state) => state.newChatDraft?.harnessId ?? undefined),
  );
  return (
    <ComposerOptionsBrowser
      {...props}
      load={null}
      draftBuiltins={builtins}
      // Through the field's editing commands, so Undo restores the text.
      insertCommand={(field, { name }) =>
        void import("./ComposerCommandRuntime").then((runtime) =>
          runtime.insertSlashCommand(field, name),
        )
      }
    />
  );
}

const CatalogPart = lazyParts<
  Pick<typeof CommandRuntime, "ComposerCommandCatalog">
>(() => import("./ComposerCommandRuntime"));
export function ComposerSectionMenu(props: ComposerAddActionsProps) {
  const pinnedOwner = usePinnedAgentChatOwner();
  const owner = props.boundChatId ?? pinnedOwner ?? undefined;
  return (
    <CatalogPart
      part="ComposerCommandCatalog"
      boundChatId={owner}
      fallback={<ComposerSectionRows {...props} load={{ kind: "loading" }} />}
    >
      {(load) => <ComposerSectionRows {...props} load={load} />}
    </CatalogPart>
  );
}

export function ComposerOptionsMenu(props: ComposerOptionsProps) {
  const pinnedOwner = usePinnedAgentChatOwner();
  const owner = props.boundChatId ?? pinnedOwner ?? undefined;
  // No session has a catalog yet, and another chat's must not show.
  if (props.draftPickers) return <DraftOptionsBrowser {...props} />;
  return (
    <CatalogPart
      part="ComposerCommandCatalog"
      boundChatId={owner}
      fallback={
        <ComposerMenuLoading
          triggerRef={props.triggerRef}
          onDismiss={props.onDismiss}
        />
      }
    >
      {(load, insertCommand) => (
        <ComposerOptionsBrowser
          {...props}
          load={load}
          insertCommand={insertCommand}
        />
      )}
    </CatalogPart>
  );
}
