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
import { lazyParts } from "./lazyParts";

const Usage = lazyParts<typeof LimitsChunk>(
  () => import("./ComposerUsageLimits"),
);

/** The chat's running session, as the native reads name it. */
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

/** What `/status` and `/mcp` open beside the composer. */
export function ComposerCommandPanel({
  kind,
  boundChatId,
  fieldRef,
  onClose,
}: {
  kind: AcpCommandPanel;
  boundChatId?: string;
  fieldRef: RefObject<HTMLTextAreaElement | null>;
  onClose: () => void;
}) {
  const id = useId();
  const page = useStore(
    acpProjectionStoreFor(boundChatId),
    (state) => state.page,
  );
  // Read once per opening: a session that changes closes nothing, but a
  // new runtime is named by the next opening.
  const [request] = useState(() => liveRequest(boundChatId, page));
  return (
    <MenuPopover
      id={id}
      role="dialog"
      ariaLabel={kind === "mcp" ? "MCP servers" : "Session status"}
      className="composer__popover composer-command-panel"
      placement="above-start"
      open
      triggerRef={fieldRef}
      onDismiss={onClose}
    >
      {kind === "mcp" ? (
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
