import { useId, useState, type RefObject } from "react";
import { useStore } from "zustand";
import "../../styles/composer-command-panel.css";
import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import { useChatAccessStore } from "../../app/sessionAccessActions";
import { useSessionAccessModeStore } from "../../app/sessionAccessModeStore";
import { useWorkbenchStore } from "../../app/store";
import type { AcpChatPage } from "../../domain/acp";
import type { AcpCommandPanel } from "../../domain/acpCommand";
import type { AcpUsageRequest } from "../../domain/acpUsage";
import { MenuPopover } from "../../ui/MenuPopover";
import { currentSession } from "./acp/acpTurnState";
import { currentChatUsage } from "./acp/chatUsage";
import { currentContext } from "./acp/useAcpChatActions";
import { chatAccessCandidates, chatAccessEffective } from "./chatAccessMode";
import { confirmedComposerModel } from "./inheritedComposerModel";
import { ComposerMcpServers } from "./ComposerMcpServers";
import type * as LimitsChunk from "./ComposerUsageLimits";
import type * as ExtensionsChunk from "./ComposerExtensions";
import type { AvailableAcpCommand } from "../../services/acpCommandsDesktop";
import { lazyParts } from "./lazyParts";

const Usage = lazyParts<typeof LimitsChunk>(
  () => import("./ComposerUsageLimits"),
);
const Extensions = lazyParts<typeof ExtensionsChunk>(
  () => import("./ComposerExtensions"),
);

/** Command panels, plus the views only the + menu opens. */
export type ComposerPanelKind = AcpCommandPanel | "managedMcp" | "extensions";

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
  onRunCommand,
}: {
  kind: ComposerPanelKind;
  boundChatId?: string;
  fieldRef: RefObject<HTMLTextAreaElement | null>;
  onClose: () => void;
  onRunCommand?: (command: AvailableAcpCommand) => void;
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
        <Extensions
          part="ManagedMcpPanel"
          boundChatId={boundChatId}
          page={page}
          onRunCommand={onRunCommand}
        />
      ) : kind === "mcp" ? (
        <ComposerMcpServers request={request} />
      ) : (
        <SessionStatus page={page} request={request} />
      )}
    </MenuPopover>
  );
}

function useAccessLabel(page: AcpChatPage | null): string | null {
  const session = page ? currentSession(page) : null;
  const config = page?.sessionConfig;
  const capabilities = useWorkbenchStore(
    (state) =>
      state.harnesses.find(
        (harness) => harness.bindingId === session?.bindingId,
      )?.accessModes,
  );
  const stored = useSessionAccessModeStore((state) =>
    session ? state.modes[session.bindingId] : undefined,
  );
  const chat = useChatAccessStore((state) =>
    config ? state.modes[config.sageAgentSessionId] : undefined,
  );
  const candidates = chatAccessCandidates(
    capabilities,
    config?.modes?.availableModes.map((mode) => mode.id) ?? null,
  );
  const effective = chatAccessEffective(stored, candidates, chat);
  return candidates.find(({ id }) => id === effective)?.label ?? null;
}

function SessionStatus({
  page,
  request,
}: {
  page: AcpChatPage | null;
  request: AcpUsageRequest | null;
}) {
  const model = confirmedComposerModel(page?.sessionConfig);
  const access = useAccessLabel(page);
  const context = currentChatUsage(page);
  const rows = (
    [
      ["Model", model?.model],
      ["Effort", model?.effort],
      ["Access", access],
      [
        "Context",
        context?.contextWindow
          ? `${context.totalTokens.toLocaleString()} / ${context.contextWindow.toLocaleString()} tokens`
          : null,
      ],
    ] satisfies [string, string | null | undefined][]
  ).filter((row): row is [string, string] => Boolean(row[1]));
  return (
    <>
      {rows.length ? (
        <dl className="composer-command-panel__status">
          {rows.map(([term, value]) => (
            <div key={term}>
              <dt>{term}</dt>
              <dd>{value}</dd>
            </div>
          ))}
        </dl>
      ) : (
        <p role="status">Status appears once the session is running.</p>
      )}
      {request && page?.usage ? (
        <div className="composer-usage">
          <Usage part="UsageReport" request={request} />
        </div>
      ) : null}
    </>
  );
}
