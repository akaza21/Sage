import { acpProjectionStoreFor } from "../../../src/app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../../src/app/store";
import type {
  AcpChatPage,
  SageChatId,
  SageItemId,
  SageTurnId,
  ProjectionRevision,
  AcpProjectionChangedEvent,
} from "../../../src/domain/acp";
import type { BootstrapSnapshot } from "../../../src/domain/workbench";
import { getWorkbenchPorts } from "../../../src/ports/workbenchRuntime";
import { acpDesktop } from "../../../src/services/acpDesktop";
import { buildVisualScenario } from "./visualScenarioShell";

export const selectionAnswer =
  "The preceding explanation introduces the selected phrase and its behavior for null.\n\nA second paragraph provides another selection for review.";

export function selectionAnswerText(longAnswer: boolean): string {
  return (
    selectionAnswer +
    (longAnswer
      ? "\n\n" +
        Array.from(
          { length: 40 },
          (_, index) =>
            `Additional paragraph ${index + 1} makes room for a flipped selection toolbar.`,
        ).join("\n\n")
      : "")
  );
}

let completeSide: (() => void) | null = null;
export function completeSideChatFixture() {
  if (!completeSide) throw new Error("Send a side chat prompt first");
  completeSide();
}

export function installTranscriptSelectionFixture(
  longAnswer = false,
  forkable = false,
) {
  const source = buildVisualScenario("rich").pages[0];
  if (forkable)
    source.sessions = source.sessions.map((session) =>
      session.id === source.currentSessionId
        ? {
            ...session,
            residency: "hot",
            capabilities: { ...session.capabilities, nativeSessionFork: true },
          }
        : session,
    );
  const turn = source.turns[0];
  const answer = turn.items.find(
    (item) => item.kind === "text" && item.payload.role === "assistant",
  );
  if (!answer || answer.kind !== "text")
    throw new Error("Missing assistant fixture");
  answer.payload.text = selectionAnswerText(longAnswer);
  answer.lifecycle = "completed";
  turn.state = "completed";
  turn.items = turn.items.filter((item) => item.kind === "text");
  turn.activities = [];
  source.turns = [turn];
  const pages = new Map<string, AcpChatPage>([[source.chatId, source]]);
  const listeners = new Set<(event: AcpProjectionChangedEvent) => void>();
  acpDesktop.subscribeProjectionChanges = async (listener) => {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  };
  const publish = (next: AcpChatPage) => {
    pages.set(next.chatId, next);
    const store = acpProjectionStoreFor(next.chatId).getState();
    if (store.scope)
      store.applyAuthoritativePage(store.scope, structuredClone(next));
    if (next.currentSessionId)
      for (const listener of listeners)
        listener({
          projectId: next.projectId,
          chatId: next.chatId,
          sessionId: next.currentSessionId,
          projectionRevision: next.projectionRevision,
        });
  };
  acpDesktop.getChatPage = async ({ chatId }) => {
    const page = pages.get(chatId);
    if (!page) throw new Error("Unknown fixture chat");
    return structuredClone(page);
  };
  acpProjectionStoreFor(source.chatId)
    .getState()
    .applyAuthoritativePage(
      {
        projectId: source.projectId,
        projectGeneration: "0" as never,
        chatId: source.chatId,
      },
      structuredClone(source),
    );
  useWorkbenchStore.setState({
    activateTab: async (id) => {
      useWorkbenchStore.setState({ activeTabId: id });
    },
  });
  const snapshot = (transitionEpoch: number): BootstrapSnapshot => {
    const state = useWorkbenchStore.getState();
    return {
      transitionEpoch,
      projects: state.projects,
      project: state.project,
      tabs: state.tabs,
      activeTabId: state.activeTabId,
      selectedRootId: state.selectedRootId,
      selectedResource: state.selectedResource,
      harnesses: state.harnesses,
      sessionId: null,
      selectedHarnessId: state.selectedHarnessId,
      selectedModelId: state.selectedModelId,
      messages: [],
      repositories: state.repositories,
      turnChanges: [],
      recoveredBuffers: [],
      recoveryNotice: null,
      archivedChats: [],
    };
  };
  let sideSequence = 0;
  const ports = getWorkbenchPorts();
  ports.chat.createChat = async (layout, expected) => {
    const id = `side-fixture-${++sideSequence}` as SageChatId;
    pages.set(id, {
      ...structuredClone(source),
      chatId: id,
      title: "Side chat",
      turns: [],
    });
    const tab = {
      id,
      kind: "chat" as const,
      title: "Side chat",
      position: layout.tabs.length,
      pinned: false,
      dirty: false,
    };
    return {
      committed: true,
      transitionEpoch: expected + 1,
      recoveryError: null,
      snapshot: {
        ...snapshot(expected + 1),
        tabs: [...layout.tabs, tab],
        activeTabId: id,
      },
    };
  };
  // A browser stand-in for Rust `fork_chat`: the fork sits above its parent,
  // is titled "<parent> (2)" and carries the lineage the divider reads.
  ports.chat.forkChat = async ({
    currentLayout,
    chatId,
    turnId,
    expectedTransitionEpoch,
  }) => {
    const parent = currentLayout.tabs.find((tab) => tab.id === chatId);
    if (!parent) throw new Error("Unknown fixture parent");
    const id = "fork-fixture-1" as SageChatId;
    const title = `${parent.title} (2)`;
    pages.set(id, {
      ...structuredClone(source),
      chatId: id,
      title,
      turns: [],
      forkedFrom: chatId as SageChatId,
      forkedFromTurnId: turnId as SageTurnId,
    });
    const tabs = currentLayout.tabs.flatMap((tab) =>
      tab.id === chatId
        ? [
            { ...tab, id, title },
            { ...tab, position: tab.position + 1 },
          ]
        : [
            tab.position > parent.position
              ? { ...tab, position: tab.position + 1 }
              : tab,
          ],
    );
    return {
      committed: true,
      transitionEpoch: expectedTransitionEpoch + 1,
      recoveryError: null,
      snapshot: {
        ...snapshot(expectedTransitionEpoch + 1),
        tabs,
        activeTabId: id,
      },
    };
  };
  ports.project.persistProjectLayout = async () => undefined;
  ports.project.getBootstrapSnapshot = async () =>
    snapshot(useWorkbenchStore.getState().nativeTransitionEpoch);
  acpDesktop.sendTurn = async (request) => {
    const current = pages.get(request.chatId)!;
    const next = structuredClone(current);
    const original = source.turns[0];
    const user = original.items.find(
      (item) => item.kind === "text" && item.payload.role === "user",
    );
    if (!user || user.kind !== "text") throw new Error("Missing user fixture");
    const sent = structuredClone(user);
    sent.id = `selection-user-${next.turns.length}` as SageItemId;
    sent.chatId = next.chatId;
    sent.payload.text = request.prompt;
    const followup = {
      ...structuredClone(original),
      id: `selection-turn-${next.chatId}-${next.turns.length}` as SageTurnId,
      // After every retained Turn, so the sent Turn renders last.
      ordinal: (next.turns.at(-1)?.ordinal ?? 0) + 1,
      createdAt: (next.turns.at(-1)?.createdAt ?? original.createdAt) + 1_000,
      attachedContext: [...(request.attachedContext ?? [])],
      items: [sent],
      activities: [],
    };
    sent.turnId = followup.id;
    if (next.chatId.startsWith("side-")) {
      followup.state = "running";
      completeSide = () => {
        const completed = structuredClone(next);
        const response = structuredClone(answer);
        response.id = `selection-answer-${next.chatId}` as SageItemId;
        response.chatId = next.chatId;
        response.turnId = followup.id;
        response.ordinal = 1;
        response.lifecycle = "completed";
        response.payload.text =
          "This is a temporary discussion of the selected phrase.";
        completed.turns = [
          { ...followup, state: "completed", items: [sent, response] },
        ];
        completed.projectionRevision = String(
          Number(next.projectionRevision) + 1,
        ) as ProjectionRevision;
        publish(completed);
      };
    }
    next.turns = [...next.turns, followup];
    next.projectionRevision = String(
      Number(next.projectionRevision) + 1,
    ) as ProjectionRevision;
    publish(next);
    return { projection: structuredClone(next) };
  };
}
