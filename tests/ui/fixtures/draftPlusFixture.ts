import { acpProjectionStoreFor } from "../../../src/app/acpProjectionRegistry";
import { useSessionAccessModeStore } from "../../../src/app/sessionAccessModeStore";
import { useWorkbenchStore } from "../../../src/app/store";
import type { AcpChatPage, ProjectionRevision } from "../../../src/domain/acp";
import type { SendAcpTurnRequest } from "../../../src/services/acpDesktop";
import { acpDesktop } from "../../../src/services/acpDesktop";
import { acpCommandsDesktop } from "../../../src/services/acpCommandsDesktop";

const MAX_CAPTURED = 16;

const deferred = () => {
  let release = () => {};
  const promise = new Promise<void>((resolve) => (release = resolve));
  return { promise, release };
};

/**
 * On the agent-single scenario: gives its ACP harness a Plan access mode,
 * answers native image uploads and chat access writes, and lets a new-chat
 * draft create a chat with no session. Its first send starts a session and
 * sends a turn; both are captured on `window.draftPlus`, with switches to
 * hold or fail uploads and to hold the created chat's page.
 */
export function installDraftPlusFixture() {
  useSessionAccessModeStore.setState({ ready: true });
  const state = useWorkbenchStore.getState();
  const chat = state.tabs.find((tab) => tab.kind === "chat");
  const source = chat && acpProjectionStoreFor(chat.id).getState().page;
  const session = source?.sessions.find(
    (item) => item.id === source.currentSessionId,
  );
  if (!source || !session)
    throw new Error("Draft fixture requires a loaded chat session");
  const control = {
    creates: 0,
    started: 0,
    chatId: null as string | null,
    uploads: [] as string[],
    failUpload: null as string | null,
    uploadHold: null as ReturnType<typeof deferred> | null,
    pageHold: null as ReturnType<typeof deferred> | null,
    sessions: 0,
    sent: [] as SendAcpTurnRequest[],
    access: [] as unknown[],
    // Curated built-ins the draft lists, and the sessions' command catalog:
    // null until a harness reports one (revision 0); undefined leaves the
    // desktop command port alone.
    builtins: [] as string[],
    // A reported list the draft offers in place of the built-ins.
    commands: null as unknown[] | null,
    catalog: undefined as string[] | null | undefined,
    ran: [] as string[],
    lists: 0,
    listHold: null as ReturnType<typeof deferred> | null,
    holdUploads() {
      control.uploadHold = deferred();
    },
    holdPage() {
      control.pageHold = deferred();
    },
    holdList() {
      control.listHold = deferred();
    },
  };
  Object.assign(window, { draftPlus: control });
  const bounded = (list: unknown[]) => {
    if (list.length >= MAX_CAPTURED) throw new Error("Capture limit reached");
  };
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
      control.creates += 1;
      const id = chatId ?? crypto.randomUUID();
      control.chatId = id;
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
    // As the workbench does: activating any other tab, or leaving a shown
    // draft, cancels a pending first send.
    activateTab: async (id, submission) => {
      const current = useWorkbenchStore.getState();
      useWorkbenchStore.setState({
        activeTabId: id,
        ...(current.pendingFirstSend !== submission && {
          pendingFirstSend: null,
          agentCenter: "chat" as const,
        }),
      });
    },
    // A project-less target keeps the chat where it is, as the internal
    // project, so a retargeted draft can still hand off.
    setChatProject: async (target) => {
      const current = useWorkbenchStore.getState();
      if (target === null && current.project)
        useWorkbenchStore.setState({
          project: { ...current.project, isInternal: true },
        });
    },
  });
  const pages = new Map<string, AcpChatPage>([[source.chatId, source]]);
  // A created chat has no session until its first send starts one.
  const pageFor = (chatId: string) =>
    pages.get(chatId) ??
    pages
      .set(chatId, {
        ...structuredClone(source),
        chatId,
        sessions: [],
        currentSessionId: null,
        turns: [],
      } as never)
      .get(chatId)!;
  const revised = (chatId: string, change: (page: AcpChatPage) => void) => {
    const next = structuredClone(pageFor(chatId));
    change(next);
    next.projectionRevision = String(
      Number(next.projectionRevision) + 1,
    ) as ProjectionRevision;
    pages.set(chatId, next);
    return { projection: structuredClone(next) } as never;
  };
  const loadPage = acpDesktop.getChatPage;
  acpDesktop.getChatPage = async (request, ...rest) => {
    if (request.chatId === source.chatId) return loadPage(request, ...rest);
    await control.pageHold?.promise;
    return structuredClone(pageFor(request.chatId));
  };
  acpDesktop.startSession = async (request) => {
    if (control.sessions >= MAX_CAPTURED)
      throw new Error("Capture limit reached");
    control.sessions += 1;
    return revised(request.chatId, (page) => {
      page.sessions = [structuredClone(session)];
      page.currentSessionId = session.id;
    });
  };
  acpDesktop.sendTurn = async (request) => {
    bounded(control.sent);
    control.sent.push(structuredClone(request));
    return revised(request.chatId, () => {});
  };
  const { listCommands, runCommand } = acpCommandsDesktop;
  acpCommandsDesktop.listCommands = async (request) => {
    if (control.catalog === undefined) return listCommands(request);
    control.lists += 1;
    await control.listHold?.promise;
    return {
      revision: control.catalog ? 1 : 0,
      commands: (control.catalog ?? []).map((name) => ({
        name,
        description: "",
        inputHint: null,
        source: "builtin" as const,
      })),
    };
  };
  acpCommandsDesktop.runCommand = async (request) => {
    if (control.catalog === undefined) return runCommand(request);
    bounded(control.ran);
    control.ran.push(request.name);
    return revised(request.chatId, () => {});
  };
  Object.defineProperty(window, "__TAURI_EVENT_PLUGIN_INTERNALS__", {
    configurable: true,
    writable: true,
    value: { unregisterListener: () => undefined },
  });
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    configurable: true,
    value: {
      transformCallback: () => 0,
      invoke: async (
        command: string,
        args: {
          request?: { label?: string; dataBase64?: string; mode?: string };
        },
      ) => {
        if (command === "list_draft_commands")
          return (
            control.commands ??
            control.builtins.map((name) => ({
              name,
              description: "Built-in",
              inputHint: null,
              source: "builtin",
              placement: { section: "skillsAndCommands" },
            }))
          );
        if (command === "set_chat_access") {
          bounded(control.access);
          control.access.push(args.request);
          return null;
        }
        if (command !== "upload_chat_image") return null;
        const label = args.request?.label ?? "image.png";
        control.started += 1;
        await control.uploadHold?.promise;
        if (control.failUpload === label) {
          control.failUpload = null;
          throw new Error(`Could not upload ${label}.`);
        }
        bounded(control.uploads);
        control.uploads.push(label);
        return {
          kind: "image",
          artifactId: `5a1f0c3e-2b4d-4c6e-8f10-${String(control.uploads.length).padStart(12, "0")}`,
          mediaType: "image/png",
          width: 1,
          height: 1,
          byteLength: atob(args.request?.dataBase64 ?? "").length,
          sha256: "b".repeat(64),
          label,
        };
      },
    },
  });
}
