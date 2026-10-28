import { useWorkbenchStore } from "../app/store";
import { createChatTransitionContext } from "../app/chatTransitions";
import { currentProjectLayout } from "../app/projectProjection";
import { acpProjectionStoreFor } from "../app/acpProjectionRegistry";
import {
  nextProjectGeneration,
  validateTransitionIntent,
} from "../app/projectIntent";
import type { TranscriptQuote } from "../domain/transcriptQuote";
import type { ProjectTransitionResult } from "../domain/workbench";
import { quoteContext } from "../features/chat/transcriptSelection";
import {
  confirmedComposerModel,
  inheritComposerModel,
} from "../features/chat/inheritedComposerModel";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { invokeNative, isTauriRuntime } from "../ports/nativeInvoke";
import {
  MAX_SIDE_CHATS,
  SIDE_CHAT_SPLIT,
  useSideChatState,
  type SideChatEntry,
} from "./sideChatState";

/** Side chats share parent Project/harness/model and attach the selection. Rust owns lifecycle; first send starts a new session, requiring no fork capability. */
let pending = false;
/** Fixtures lack Rust preferences; native uses settings. */
let confirmInBrowser = true;
let replacement: { parent: string; quote: TranscriptQuote } | null = null;

function rememberSideChat(side: SideChatEntry) {
  const sides = useSideChatState
    .getState()
    .sides.filter((entry) => entry.projectId !== side.projectId);
  if (sides.length >= MAX_SIDE_CHATS)
    throw new Error("Up to eight Projects may own a side chat");
  useSideChatState.setState({
    sides: [...sides, side],
    split: SIDE_CHAT_SPLIT,
  });
}

async function applyTransition(
  result: ProjectTransitionResult,
  expected: number,
) {
  validateTransitionIntent(result, expected);
  if (!result.committed)
    throw new Error(result.recoveryError ?? "Side chat change did not commit");
  const context = createChatTransitionContext(
    useWorkbenchStore.setState,
    useWorkbenchStore.getState,
  );
  const generation = nextProjectGeneration(
    useWorkbenchStore.getState().projectGeneration,
  );
  useWorkbenchStore.setState({ projectGeneration: generation });
  const snapshot =
    result.snapshot ??
    (await getWorkbenchPorts().project.getBootstrapSnapshot());
  if (snapshot.transitionEpoch !== result.transitionEpoch)
    throw new Error("Side chat projection has a different owner epoch");
  await context.applyProjection(snapshot, generation);
  return snapshot;
}

/** Failed IPC may commit: adopt only an authoritative snapshot with exactly one expected tab transition. */
async function nativeTransition(
  command: string,
  args: Record<string, unknown>,
  expected: number,
  committed: (tabs: readonly { id: string }[]) => boolean,
): Promise<ProjectTransitionResult> {
  try {
    return await invokeNative<ProjectTransitionResult>(command, args);
  } catch (error) {
    const snapshot = await getWorkbenchPorts().project.getBootstrapSnapshot();
    if (snapshot.transitionEpoch !== expected + 1 || !committed(snapshot.tabs))
      throw error;
    return {
      committed: true,
      transitionEpoch: snapshot.transitionEpoch,
      snapshot,
      recoveryError: null,
    };
  }
}

async function exclusive<T>(work: () => Promise<T>): Promise<T> {
  if (pending || useWorkbenchStore.getState().projectTransitioning)
    throw new Error("Wait for the current Project operation");
  pending = true;
  useWorkbenchStore.setState({ projectTransitioning: true });
  try {
    return await work();
  } finally {
    pending = false;
    useWorkbenchStore.setState({ projectTransitioning: false });
  }
}

export async function openSideChat(
  parentChatId: string,
  quote: TranscriptQuote,
) {
  const state = useWorkbenchStore.getState();
  const layout = currentProjectLayout(state);
  if (!layout) throw new Error("Open a Project before starting a side chat");
  const existing = useSideChatState
    .getState()
    .sides.find((entry) => entry.projectId === layout.projectId);
  if (existing) {
    // One side chat per Project: replace it once the user confirms the close.
    replacement = {
      parent:
        parentChatId === existing.id ? existing.parentChatId : parentChatId,
      quote,
    };
    try {
      await requestCloseSideChat(existing.id);
    } catch (error) {
      replacement = null;
      throw error;
    }
    return;
  }
  const source = acpProjectionStoreFor(parentChatId).getState().page;
  const session = source?.sessions.find(
    (entry) => entry.id === source.currentSessionId,
  );
  const parentUi = state.chatUiById[parentChatId];
  const harnessId = session
    ? state.harnesses.find(
        (harness) =>
          harness.transport === "acp" &&
          harness.bindingId === session.bindingId,
      )?.id
    : (parentUi?.harnessId ?? state.selectedHarnessId);
  const modelId =
    confirmedComposerModel(source?.sessionConfig)?.id ??
    session?.modelId ??
    parentUi?.modelId ??
    null;
  if (!harnessId) throw new Error("Select a harness before opening side chat");
  const expected = state.nativeTransitionEpoch;
  const ownershipEpoch = state.chatOwnershipEpoch;
  const id = await exclusive(async () => {
    const result = isTauriRuntime()
      ? await nativeTransition(
          "open_side_chat",
          {
            currentLayout: layout,
            parentChatId,
            expectedTransitionEpoch: expected,
          },
          expected,
          (tabs) =>
            tabs.some(
              (tab) =>
                tab.id.startsWith("side-") &&
                !layout.tabs.some((prior) => prior.id === tab.id),
            ),
        )
      : await getWorkbenchPorts().chat.createChat(layout, expected);
    if (useWorkbenchStore.getState().chatOwnershipEpoch !== ownershipEpoch)
      throw new Error("Project changed while opening side chat");
    const snapshot = await applyTransition(result, expected);
    const created = snapshot.activeTabId;
    if (!created || layout.tabs.some((tab) => tab.id === created))
      throw new Error("Side chat was not created");
    rememberSideChat({
      id: created,
      projectId: layout.projectId,
      parentChatId,
    });
    // The side session starts on `modelId` with no effort of its own.
    inheritComposerModel(created, source?.sessionConfig, {
      startsOnModelId: modelId,
    });
    return created;
  });
  const current = useWorkbenchStore.getState();
  const intent = current.captureChatOwnership(id);
  if (
    !intent ||
    !current.setChatHarnessSelection(intent, { harnessId, modelId }) ||
    !current.attachChatContext(intent, quoteContext(quote))
  )
    throw new Error("The selection could not be attached to this side chat");
}

export async function requestCloseSideChat(id: string) {
  const confirm = isTauriRuntime()
    ? await invokeNative<boolean>("get_confirm_close_side_chat")
    : confirmInBrowser;
  if (confirm) useSideChatState.setState({ pendingClose: id });
  else await closeSideChat(id, false);
}

export function cancelCloseSideChat() {
  replacement = null;
  useSideChatState.setState({ pendingClose: null });
}

export async function closeSideChat(id: string, dontAskAgain: boolean) {
  // The pending replacement belongs to this close attempt only.
  const next = replacement;
  replacement = null;
  const state = useWorkbenchStore.getState();
  const side = useSideChatState
    .getState()
    .sides.find((entry) => entry.id === id);
  if (!side || state.project?.id !== side.projectId)
    throw new Error(
      "Side chat owner changed; return to its Project before closing",
    );
  const expected = state.nativeTransitionEpoch;
  await exclusive(async () => {
    if (isTauriRuntime()) {
      // Rust cancels the Turn, ends runtime and deletes Chat.
      const result = await nativeTransition(
        "close_side_chat",
        {
          projectId: side.projectId,
          chatId: id,
          expectedTransitionEpoch: expected,
          dontAskAgain,
        },
        expected,
        (tabs) => !tabs.some((tab) => tab.id === id),
      );
      await applyTransition(result, expected);
    } else {
      const layout = currentProjectLayout(state);
      if (!layout) throw new Error("Open a Project before closing side chat");
      await getWorkbenchPorts().project.persistProjectLayout(
        {
          ...layout,
          tabs: layout.tabs.filter((tab) => tab.id !== id),
          activeTabId: side.parentChatId,
        },
        expected,
      );
      useWorkbenchStore.setState({
        tabs: state.tabs.filter((tab) => tab.id !== id),
        activeTabId: side.parentChatId,
      });
      if (dontAskAgain) confirmInBrowser = false;
    }
    useSideChatState.setState((current) => ({
      sides: current.sides.filter((entry) => entry.id !== id),
      pendingClose: null,
    }));
  });
  if (next) await openSideChat(next.parent, next.quote);
  else
    queueMicrotask(() =>
      document.getElementById(`prompt-${side.parentChatId}`)?.focus(),
    );
}
