import { useWorkbenchStore } from "../../app/store";
import { useChatAccessStore } from "../../app/sessionAccessActions";
import { useSessionAccessModeStore } from "../../app/sessionAccessModeStore";
import type { AcpChatPage } from "../../domain/acp";
import type { AcpUsageRequest } from "../../domain/acpUsage";
import { currentSession } from "./acp/acpTurnState";
import { currentChatUsage } from "./acp/chatUsage";
import { chatAccessCandidates, chatAccessEffective } from "./chatAccessMode";
import { confirmedComposerModel } from "./inheritedComposerModel";
import type * as LimitsChunk from "./ComposerUsageLimits";
import { lazyParts } from "./lazyParts";
const Usage = lazyParts<typeof LimitsChunk>(
  () => import("./ComposerUsageLimits"),
);
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

export function SessionStatus({
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
