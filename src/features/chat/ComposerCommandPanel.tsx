import { useId, useState, type RefObject } from "react";
import { useStore } from "zustand";
import "../../styles/composer-command-panel.css";
import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import type { AcpChatPage } from "../../domain/acp";
import type { AcpCommandPanel } from "../../domain/acpCommand";
import type { AcpUsageRequest } from "../../domain/acpUsage";
import { MenuPopover } from "../../ui/MenuPopover";
import { currentSession } from "./acp/acpTurnState";
import { currentContext } from "./acp/useAcpChatActions";
import { ComposerMcpServers } from "./ComposerMcpServers";
import { lazyParts } from "./lazyParts";
import type * as ExtensionsChunk from "./ComposerExtensions";
const Extensions = lazyParts<typeof ExtensionsChunk>(
  () => import("./ComposerExtensions"),
);

import type * as StatusChunk from "./ComposerSessionStatus";
const Status = lazyParts<typeof StatusChunk>(
  () => import("./ComposerSessionStatus"),
);

function liveRequest(
  boundChatId: string | undefined,
  page: AcpChatPage | null,
): AcpUsageRequest | null {
  const session = page ? currentSession(page) : null;
  const context = currentContext(boundChatId);
  if (!session?.runtimeEpoch || session.residency !== "hot" || !context)
    return null;
  return {
    ...context.scope,
    projectGeneration: context.commandProjectGeneration,
    sessionId: session.id,
    runtimeEpoch: session.runtimeEpoch,
  };
}

export function ComposerCommandPanel({
  kind,
  boundChatId,
  fieldRef,
  onClose,
}: {
  kind: AcpCommandPanel | "extensions";
  boundChatId?: string;
  fieldRef: RefObject<HTMLTextAreaElement | null>;
  onClose: () => void;
}) {
  const id = useId();
  const page = useStore(
    acpProjectionStoreFor(boundChatId),
    (state) => state.page,
  );
  // Keep the runtime captured at open; reopening picks a new one.
  const [request] = useState(() => liveRequest(boundChatId, page));
  return (
    <MenuPopover
      id={id}
      role="dialog"
      ariaLabel={
        kind === "extensions"
          ? "Extensions"
          : kind === "status"
            ? "Session status"
            : "MCP servers"
      }
      className="composer__popover composer-command-panel"
      placement="above-start"
      open
      triggerRef={fieldRef}
      onDismiss={onClose}
    >
      {kind === "extensions" ? (
        <Extensions
          part="ExtensionsPanel"
          boundChatId={boundChatId}
          page={page}
        />
      ) : kind === "managedMcp" ? (
        <Extensions part="ManagedMcpPanel" boundChatId={boundChatId} />
      ) : kind === "mcp" ? (
        <ComposerMcpServers request={request} />
      ) : (
        <Status part="SessionStatus" page={page} request={request} />
      )}
    </MenuPopover>
  );
}
