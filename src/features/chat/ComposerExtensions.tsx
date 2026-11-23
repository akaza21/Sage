import { useId, useState, type RefObject } from "react";
import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../app/store";
import type { AcpChatPage } from "../../domain/acp";
import { flattenChoices } from "../../domain/acpSessionConfigControls";
import type { AvailableAcpCommand } from "../../services/acpCommandsDesktop";
import { MenuPopover } from "../../ui/MenuPopover";
import type { SessionCommandsLoad } from "./useSessionCommands";

/**
 * The + menu's Extensions and managed MCP views. The menu hands in its own
 * command load, so this chunk keeps no catalog subscription or cache.
 */
export function ExtensionsPopover({
  kind,
  load,
  boundChatId,
  triggerRef,
  onClose,
  onRunCommand,
}: {
  kind: "extensions" | "managedMcp";
  load: SessionCommandsLoad | null;
  boundChatId?: string;
  triggerRef: RefObject<HTMLTextAreaElement | null>;
  onClose: () => void;
  onRunCommand: (command: AvailableAcpCommand) => void;
}) {
  const id = useId();
  // A snapshot taken on open: streaming turns do not re-render this view.
  const [page] = useState(
    () => acpProjectionStoreFor(boundChatId).getState().page,
  );
  return (
    <MenuPopover
      id={id}
      role="dialog"
      ariaLabel={kind === "extensions" ? "Extensions" : "MCP servers"}
      className="composer__popover composer__options"
      placement="above-start"
      open
      triggerRef={triggerRef}
      onDismiss={onClose}
    >
      {load?.kind !== "ready" ? (
        <p role="status">
          {load?.kind === "error" ? load.message : "Loading commands…"}
        </p>
      ) : kind === "extensions" ? (
        <ComposerExtensions
          catalog={load.list.commands}
          page={page}
          omitted={load.list.omitted}
        />
      ) : (
        <ManagedMcp
          catalog={load.list.commands}
          page={page}
          onRunCommand={onRunCommand}
        />
      )}
    </MenuPopover>
  );
}

/** The harness's own MCP command: placed on the MCP row, with no Sage panel. */
const managedMcp = (catalog: readonly AvailableAcpCommand[]) =>
  catalog.find(
    (command) => !command.panel && command.placement?.control === "mcpPanel",
  );

function useHarnessName(page: AcpChatPage | null): string | undefined {
  const binding = page?.sessions.find(
    (session) => session.id === page.currentSessionId,
  )?.bindingId;
  return useWorkbenchStore(
    (state) =>
      state.harnesses.find((entry) => entry.bindingId === binding)?.name,
  );
}

/** The harness reports no MCP inventory over its protocol; its command does. */
function ManagedMcp({
  catalog,
  page,
  onRunCommand,
}: {
  catalog: readonly AvailableAcpCommand[];
  page: AcpChatPage | null;
  onRunCommand: (command: AvailableAcpCommand) => void;
}) {
  const harness = useHarnessName(page) ?? "the harness";
  const managed = managedMcp(catalog);
  if (!managed)
    return <p role="status">MCP servers are unavailable in this session.</p>;
  return (
    <section aria-label="MCP servers">
      <strong>MCP servers</strong>
      <p role="status">
        Managed by {harness}. Run <code>/{managed.name}</code> in this chat to
        print server status. MCP tools appear as ordinary tools.
      </p>
      <button
        type="button"
        className="menu-item"
        onClick={() => onRunCommand(managed)}
      >
        Insert /{managed.name}
      </button>
    </section>
  );
}

interface Row {
  key: string;
  label: string;
  detail: string;
  attributed: boolean;
}

/** Disposable search results, at most 100 rows. No inventory cache or hidden view. */
export function ComposerExtensions({
  catalog,
  page,
  omitted = 0,
}: {
  catalog: readonly AvailableAcpCommand[];
  page: AcpChatPage | null;
  omitted?: number;
}) {
  const [query, setQuery] = useState("");
  const harness = useHarnessName(page) ?? "harness";
  const search = query.trim().toLocaleLowerCase();
  const rows: Row[] = [];
  const seen = new Set<string>();
  let more = false;
  const offer = (row: Row) => {
    if (
      seen.has(row.key) ||
      !`${row.label} ${row.detail}`.toLocaleLowerCase().includes(search)
    )
      return;
    if (rows.length === 100) {
      more = true;
      return;
    }
    seen.add(row.key);
    rows.push(row);
  };
  for (const command of catalog) {
    const info = command.sourceInfo;
    if (
      info &&
      // Extension commands, package resources, and extensions the harness
      // ships itself (listed as built in, with their own provenance).
      (command.source === "extension" ||
        command.source === "builtin" ||
        info.origin === "package")
    ) {
      const key = info.path ?? info.baseDir ?? command.name;
      offer({
        key,
        label: command.sourceLabel ?? info.path ?? command.name,
        detail: [
          info.path,
          info.origin === "package" &&
            info.baseDir &&
            `Package: ${info.baseDir}`,
          info.scope,
          info.origin,
          info.source,
        ]
          .filter(Boolean)
          .join(" · "),
        attributed: true,
      });
    } else if (command.source === "extension" || command.source === "mcp") {
      offer({
        key: `command:${command.name}`,
        label: `/${command.name}`,
        detail: command.description,
        attributed: false,
      });
    }
  }
  for (const option of page?.sessionConfig?.configOptions ?? []) {
    if (option.category !== "model") continue;
    for (const model of flattenChoices(option))
      offer({
        key: `model:${model.value}`,
        label: model.name,
        detail: "Model · extension attribution unavailable",
        attributed: false,
      });
  }
  for (const turn of page?.turns ?? [])
    for (const item of turn.items) {
      if (item.kind === "toolCall")
        offer({
          key: `tool:${item.payload.presentation.label}`,
          label: item.payload.presentation.label,
          detail: "Observed tool · extension attribution unavailable",
          attributed: false,
        });
    }
  const managed = managedMcp(catalog);
  if (managed)
    offer({
      key: "mcp",
      label: "MCP servers",
      detail: `Managed by ${harness} · /${managed.name} · server inventory and extension attribution unavailable`,
      attributed: false,
    });
  return (
    <section aria-label="Extensions">
      <strong>Extensions</strong>
      <p>
        Extensions and packages reported by commands. Extensions without
        commands may not be reported.
      </p>
      <input
        className="composer-menu__search"
        type="search"
        aria-label="Search extensions"
        maxLength={256}
        value={query}
        onChange={(event) => setQuery(event.target.value.slice(0, 256))}
      />
      {[true, false].map((attributed) => (
        <section
          key={String(attributed)}
          aria-label={
            attributed ? "Extensions and packages" : `From ${harness}`
          }
        >
          <h3>{attributed ? "Extensions and packages" : `From ${harness}`}</h3>
          {!attributed ? (
            <p>
              Tools, models and MCP servers are shown here when the harness does
              not report which extension provides them. Only observed tools are
              available.
            </p>
          ) : null}
          <ul className="composer-command-panel__servers">
            {rows
              .filter((row) => row.attributed === attributed)
              .map((row) => (
                <li key={row.key}>
                  <strong>{row.label}</strong>
                  <span className="composer-command-panel__detail">
                    {row.detail}
                  </span>
                </li>
              ))}
          </ul>
        </section>
      ))}
      {!rows.length ? (
        <p role="status">No matching extension information reported.</p>
      ) : null}
      {more ? (
        <p role="status">Showing 100 rows. Refine your search to see more.</p>
      ) : null}
      {omitted > 0 ? (
        <p role="status">
          Command inventory truncated: {omitted} commands not reported to this
          view.
        </p>
      ) : null}
    </section>
  );
}
