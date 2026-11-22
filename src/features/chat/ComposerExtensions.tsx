import { useState } from "react";
import { useWorkbenchStore } from "../../app/store";
import type { AcpChatPage } from "../../domain/acp";
import { flattenChoices } from "../../domain/acpSessionConfigControls";
import type { AvailableAcpCommand } from "../../services/acpCommandsDesktop";
import { useSessionCommands } from "./useSessionCommands";

export function ExtensionsPanel({
  boundChatId,
  page,
}: {
  boundChatId?: string;
  page: AcpChatPage | null;
}) {
  const load = useSessionCommands(boundChatId);
  return load?.kind === "ready" ? (
    <ComposerExtensions
      catalog={load.list.commands}
      page={page}
      omitted={load.list.omitted}
    />
  ) : (
    <p role="status">
      {load?.kind === "error" ? load.message : "Loading extensions…"}
    </p>
  );
}

export function ManagedMcpPanel({ boundChatId }: { boundChatId?: string }) {
  const load = useSessionCommands(boundChatId);
  const managed =
    load?.kind === "ready"
      ? load.list.commands.find((command) => command.panel === "managedMcp")
      : null;
  return managed ? (
    <p role="status">
      Managed by {managed.sourceLabel ?? "the harness"}. Use{" "}
      <code>/{managed.name}</code> in this chat to inspect servers. MCP tools
      appear as ordinary tools.
    </p>
  ) : (
    <p role="status">
      {load?.kind === "error" ? load.message : "Loading MCP management…"}
    </p>
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
  const binding = page?.sessions.find(
    (session) => session.id === page.currentSessionId,
  )?.bindingId;
  const harness =
    useWorkbenchStore(
      (state) =>
        state.harnesses.find((entry) => entry.bindingId === binding)?.name,
    ) ?? "harness";
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
      (command.source === "extension" ||
        info.origin === "package" ||
        command.panel === "managedMcp")
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
  const managed = catalog.find((command) => command.panel === "managedMcp");
  if (managed)
    offer({
      key: "mcp",
      label: "MCP servers",
      detail: `Managed by ${managed.sourceLabel ?? harness} · /${managed.name} · server inventory and extension attribution unavailable`,
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
