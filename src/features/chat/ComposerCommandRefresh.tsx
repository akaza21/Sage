import { useState } from "react";
import { acpCommandCatalogDesktop } from "../../services/acpCommandCatalogDesktop";
import { commandCatalogRequest } from "./composerCommandCatalogRequest";
import "../../styles/composer-command-refresh.css";

export function ComposerCommandRefresh({
  boundChatId,
  owner,
  more,
  omitted,
  onStatusChange,
}: {
  boundChatId?: string;
  owner: string | null;
  more: number;
  omitted: number;
  onStatusChange: (status: string) => void;
}) {
  // Only the IPC call is in flight here; the harness reply arrives later
  // through the catalog change event.
  const [sending, setSending] = useState(false);
  const refresh = async () => {
    if (!owner || sending) return;
    setSending(true);
    try {
      await acpCommandCatalogDesktop.refreshCommands(
        commandCatalogRequest(owner, boundChatId),
      );
    } catch (error) {
      onStatusChange(
        error instanceof Error
          ? error.message
          : "The command list could not be refreshed.",
      );
    } finally {
      setSending(false);
    }
  };
  return (
    <>
      {more > 0 && <span>and {more} more, type to search</span>}
      {more > 0 && omitted > 0 && <br />}
      {omitted > 0 && (
        <span>Catalog truncated: {omitted} commands not shown</span>
      )}
      {owner && (
        <button
          className="composer-command-refresh"
          type="button"
          disabled={sending}
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => void refresh()}
        >
          Refresh commands
        </button>
      )}
    </>
  );
}
