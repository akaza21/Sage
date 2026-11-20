import { useWorkbenchStore } from "../../app/store";
import { CreateProjectDialog } from "../../workbench/CreateProjectDialog";
import type { ChatProjectCreation } from "./CreateChatProjectDialog";

export function ChatProjectCreationSurface({
  creating,
  onClose,
}: {
  creating: ChatProjectCreation;
  onClose: () => void;
}) {
  const owned =
    creating.draft?.submission.status === "created"
      ? creating.draft.submission.chatId
      : creating.draft
        ? null
        : creating.layout?.activeTabId;
  return (
    <CreateProjectDialog
      onClose={onClose}
      sourceChatId={owned ?? undefined}
      onCreated={async () => {
        let state = useWorkbenchStore.getState();
        const target = state.project?.id;
        if (!target) return;
        if (owned && creating.layout) {
          await state.setChatProject(
            target,
            {
              ...creating.layout,
              activeTabId: owned,
            },
            {
              ...creating.composer,
              composerText: creating.text,
              attachedContext: creating.composer?.attachedContext ?? [],
            },
          );
          state = useWorkbenchStore.getState();
          if (state.activeTabId !== owned || state.project?.id !== target) {
            // An unknown or committed recovery failure must remain blocked.
            if (state.phase === "error") return false;
            const error = state.error;
            await state.switchProject(creating.layout.projectId);
            await useWorkbenchStore.getState().activateTab(owned);
            state = useWorkbenchStore.getState();
            const owner = state.captureChatOwnership(owned);
            if (owner) {
              state.restoreChatComposer(
                owner,
                creating.text,
                creating.composer?.attachedContext ?? [],
              );
              if (creating.composer)
                state.setChatHarnessSelection(owner, {
                  harnessId: creating.composer.harnessId ?? null,
                  modelId: creating.composer.modelId ?? null,
                });
            }
            useWorkbenchStore.setState({
              error,
              ...(creating.draft
                ? { agentCenter: "draft", newChatDraft: creating.draft }
                : {}),
            });
            return false;
          }
        }
        if (creating.draft) {
          useWorkbenchStore.setState({
            agentCenter: "draft",
            newChatDraft: {
              ...creating.draft,
              projectId: target,
              targetProjectId: target,
              submission: { status: "created", chatId: state.activeTabId! },
            },
          });
        }
      }}
    />
  );
}
