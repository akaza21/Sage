import type { NewChatDraft } from "../../app/newChatDraftSlice";
import type { ProjectLayoutRequest } from "../../domain/workbench";
import { useWorkbenchStore } from "../../app/store";
import { CreateProjectDialog } from "../../workbench/CreateProjectDialog";
import type { ChatComposerDraft } from "../../domain/chatOwnership";

export interface ChatProjectCreation {
  layout: ProjectLayoutRequest | null;
  draft: NewChatDraft | null;
  text: string;
  composer?: ChatComposerDraft;
}

export function CreateChatProjectDialog({
  creating,
  onClose,
}: {
  creating: ChatProjectCreation;
  onClose: () => void;
}) {
  return (
    <CreateProjectDialog
      onClose={onClose}
      onCreated={async () => {
        let state = useWorkbenchStore.getState();
        const target = state.project?.id;
        if (!target) return;
        const owned =
          creating.draft?.submission.status === "created"
            ? creating.draft.submission.chatId
            : creating.draft
              ? null
              : creating.layout?.activeTabId;
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
          if (state.activeTabId !== owned || state.project?.id !== target)
            return;
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
