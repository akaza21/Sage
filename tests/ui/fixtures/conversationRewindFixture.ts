import { acpProjectionStoreFor } from "../../../src/app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../../src/app/store";
import type {
  AcpChatPage,
  SageTurnId,
  SageItemId,
  ProjectionRevision,
} from "../../../src/domain/acp";
import { acpDesktop } from "../../../src/services/acpDesktop";
import { installTranscriptSelectionFixture } from "./transcriptSelectionFixtures";

/** The native command boundary returns a rejection once, then commits the suffix. */
export function installConversationRewindFixture(inPlace = true) {
  installTranscriptSelectionFixture(false, true);
  const chat = useWorkbenchStore.getState().activeTabId!;
  const projection = acpProjectionStoreFor(chat);
  let page = structuredClone(projection.getState().page!) as AcpChatPage;
  const original = page.turns[0]!;
  const user = original.items.find(
    (item) => item.kind === "text" && item.payload.role === "user",
  );
  if (!user || user.kind !== "text") throw new Error("Missing prompt fixture");
  const first = {
    ...original,
    conversationTarget: true,
    id: "turn-00000000-0000-4000-8000-000000000001" as SageTurnId,
    ordinal: 0,
    terminalAt: 1,
    items: original.items.map((item) => ({
      ...item,
      turnId: "turn-00000000-0000-4000-8000-000000000001" as SageTurnId,
    })),
  };
  const second = {
    ...first,
    id: "turn-00000000-0000-4000-8000-000000000002" as SageTurnId,
    ordinal: 1,
    createdAt: first.createdAt + 1000,
    items: first.items.map((item) => ({
      ...item,
      id: `second-${item.id}` as SageItemId,
      turnId: "turn-00000000-0000-4000-8000-000000000002" as SageTurnId,
    })),
  };
  page = {
    ...page,
    turns: [first, second],
    sessions: page.sessions.map((session) => ({
      ...session,
      residency: "hot",
      capabilities: {
        ...session.capabilities,
        rewind: {
          effects: { conversation: inPlace, files: false },
          targets: "anyTurn",
          forkAt: true,
          onResume: false,
        },
      },
    })),
  };
  const publish = () => {
    const scope = projection.getState().scope;
    if (scope)
      projection
        .getState()
        .applyAuthoritativePage(scope, structuredClone(page));
  };
  publish();
  acpDesktop.getChatPage = async () => structuredClone(page);
  let refused = false;
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    configurable: true,
    value: {
      transformCallback: () => 0,
      invoke: async (
        command: string,
        args: { request?: { control?: { kind: string; before: string } } },
      ) => {
        if (command !== "control_acp_session") return null;
        const control = args.request?.control;
        if (control?.kind !== "rewindConversation")
          throw new Error("Unexpected control");
        if (!refused) {
          refused = true;
          throw { kind: "rejected", message: "Harness declined rewind." };
        }
        const target = page.turns.find((turn) => turn.id === control.before);
        if (!target) throw new Error("Unknown rewind target");
        page = {
          ...page,
          turns: page.turns.filter((turn) => turn.ordinal < target.ordinal),
          projectionRevision: String(
            Number(page.projectionRevision) + 1,
          ) as ProjectionRevision,
        };
        publish();
        return {};
      },
    },
  });
  return user.payload.text;
}
