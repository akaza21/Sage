import { useEffect, useReducer, useState, type ReactNode } from "react";
import { useStore } from "zustand";
import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import type { AcpChatPage } from "../../domain/acp";
import type { AcpCommandCatalog as AcpCommandList } from "../../services/acpCommandCatalogDesktop";
import { commandCatalogRequest } from "./composerCommandCatalogRequest";
import "../../styles/composer-command-refresh.css";

export function ComposerCommandRefresh({
  boundChatId,
  owner,
  pending,
  more,
  omitted,
  onStatusChange,
}: {
  boundChatId?: string;
  owner: string | null;
  pending: boolean;
  more: number;
  omitted: number;
  onStatusChange: (status: string) => void;
}) {
  const refresh = async () => {
    if (!owner) return;
    try {
      const { acpCommandCatalogDesktop } =
        await import("../../services/acpCommandCatalogDesktop");
      await acpCommandCatalogDesktop.refreshCommands(
        commandCatalogRequest(owner, boundChatId),
      );
    } catch (error) {
      onStatusChange(
        error instanceof Error
          ? error.message
          : "The command list could not be refreshed.",
      );
    }
  };
  return (
    <>
      {more > 0 && <span>and {more} more, type to search</span>}
      {more > 0 && omitted > 0 && <br />}
      {omitted > 0 && (
        <span>Catalog truncated: {omitted} commands not shown</span>
      )}
      {owner &&
        (pending ? (
          <span role="status">Refreshing…</span>
        ) : (
          <button
            className="composer-command-refresh"
            type="button"
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => void refresh()}
          >
            Refresh commands
          </button>
        ))}
    </>
  );
}

const LOADING: CommandLoad = { kind: "loading" };

export type CommandLoad =
  | { kind: "loading" }
  | { kind: "ready"; owner: string; list: AcpCommandList }
  | { kind: "error"; owner: string; message: string };

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

function useSessionCommands(
  boundChatId: string | undefined,
): CommandLoad | null {
  const owner = useStore(acpProjectionStoreFor(boundChatId), (state) =>
    liveOwnerKey(state.page),
  );
  const [load, setLoad] = useState<CommandLoad>(LOADING);
  const [revision, changed] = useReducer((value: number) => value + 1, 0);
  // The owner whose change listener is installed. The list is read only
  // after it, so an update that lands during the read is not missed.
  const [listening, setListening] = useState("");
  useEffect(() => {
    if (!owner) return;
    const [sessionId, runtimeEpoch] = JSON.parse(owner) as [string, string];
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void import("../../services/acpCommandCatalogDesktop")
      .then(({ acpCommandCatalogDesktop }) =>
        acpCommandCatalogDesktop.subscribeCommandChanges((event) => {
          if (
            event.sageAgentSessionId === sessionId &&
            event.runtimeEpoch === runtimeEpoch
          )
            changed();
        }),
      )
      .then(
        (dispose) => {
          if (disposed) return dispose();
          unlisten = dispose;
          setListening(owner);
        },
        () => !disposed && setListening(owner),
      );
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [owner]);
  useEffect(() => {
    if (!owner || listening !== owner) return;
    let current = true;
    void Promise.resolve()
      .then(() => {
        return import("../../services/acpCommandCatalogDesktop").then(
          ({ acpCommandCatalogDesktop }) =>
            acpCommandCatalogDesktop.listCommands(
              commandCatalogRequest(owner, boundChatId),
            ),
        );
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
                error instanceof Error
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
  return load.kind === "loading" || load.owner === owner ? load : LOADING;
}

export function ComposerCommandCatalog({
  boundChatId,
  children,
}: {
  boundChatId?: string;
  children: (load: CommandLoad | null) => ReactNode;
}) {
  return children(useSessionCommands(boundChatId));
}
