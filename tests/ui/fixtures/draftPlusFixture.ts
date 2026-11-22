import { acpProjectionStoreFor } from "../../../src/app/acpProjectionRegistry";
import { useSessionAccessModeStore } from "../../../src/app/sessionAccessModeStore";
import { useWorkbenchStore } from "../../../src/app/store";
import type { AcpChatPage, ProjectionRevision } from "../../../src/domain/acp";
import type { SendAcpTurnRequest } from "../../../src/services/acpDesktop";
import { acpDesktop } from "../../../src/services/acpDesktop";

const MAX_CAPTURED = 8;

/**
 * On the agent-single scenario: gives its ACP harness a Plan access mode,
 * answers native image uploads, and lets a new-chat draft create a chat
 * whose first turn request is captured on `window.draftPlusSent`.
 */
export function installDraftPlusFixture() {
  useSessionAccessModeStore.setState({ ready: true });
  const state = useWorkbenchStore.getState();
  const chat = state.tabs.find((tab) => tab.kind === "chat");
  const source = chat && acpProjectionStoreFor(chat.id).getState().page;
  if (!source) throw new Error("Draft fixture requires a loaded chat page");
  useWorkbenchStore.setState({
    harnesses: state.harnesses.map((harness) => ({
      ...harness,
      accessModes: [
        ...(harness.accessModes ?? []),
        {
          id: "plan" as const,
          label: "Plan",
          description: "Plan before editing.",
          sessionModeId: "plan",
        },
      ],
    })),
    createChat: async (_target, chatId) => {
      const id = chatId ?? crypto.randomUUID();
      useWorkbenchStore.setState((current) => ({
        tabs: [
          ...current.tabs,
          {
            id,
            kind: "chat",
            title: "Chat: Draft",
            position: current.tabs.length,
            pinned: false,
            dirty: false,
            status: "idle",
          },
        ],
      }));
      return { status: "confirmed", chatId: id };
    },
    activateTab: async (id) => {
      useWorkbenchStore.setState({ activeTabId: id });
    },
  });
  const pages = new Map<string, AcpChatPage>([[source.chatId, source]]);
  const pageFor = (chatId: string) =>
    pages.get(chatId) ??
    pages
      .set(chatId, { ...structuredClone(source), chatId, turns: [] } as never)
      .get(chatId)!;
  const loadPage = acpDesktop.getChatPage;
  acpDesktop.getChatPage = async (request, ...rest) =>
    request.chatId === source.chatId
      ? loadPage(request, ...rest)
      : structuredClone(pageFor(request.chatId));
  const sent: SendAcpTurnRequest[] = [];
  let uploads = 0;
  Object.assign(window, {
    draftPlusSent: sent,
    draftPlusUploads: () => uploads,
  });
  acpDesktop.sendTurn = async (request) => {
    if (sent.length >= MAX_CAPTURED) throw new Error("Capture limit reached");
    sent.push(structuredClone(request));
    const next = structuredClone(pageFor(request.chatId));
    next.projectionRevision = String(
      Number(next.projectionRevision) + 1,
    ) as ProjectionRevision;
    pages.set(next.chatId, next);
    return { projection: structuredClone(next) } as never;
  };
  // Listeners the mocked runtime hands out unregister without a native side.
  Object.defineProperty(window, "__TAURI_EVENT_PLUGIN_INTERNALS__", {
    configurable: true,
    value: { unregisterListener: () => undefined },
  });
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    configurable: true,
    value: {
      transformCallback: () => 0,
      invoke: async (
        command: string,
        args: { request?: { label?: string; dataBase64?: string } },
      ) => {
        if (command !== "upload_chat_image") return null;
        uploads += 1;
        return {
          kind: "image",
          artifactId: `5a1f0c3e-2b4d-4c6e-8f10-${String(uploads).padStart(12, "0")}`,
          mediaType: "image/png",
          width: 1,
          height: 1,
          byteLength: atob(args.request?.dataBase64 ?? "").length,
          sha256: "b".repeat(64),
          label: args.request?.label ?? "image.png",
        };
      },
    },
  });
}
