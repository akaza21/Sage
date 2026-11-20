import type { NewChatDraft } from "../../app/newChatDraftSlice";
import type { ProjectLayoutRequest } from "../../domain/workbench";
import { useWorkbenchStore } from "../../app/store";
import { CreateProjectDialog } from "../../workbench/CreateProjectDialog";

export interface ChatProjectCreation {
  layout: ProjectLayoutRequest | null;
  draft: NewChatDraft | null;
  text: string;
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
          await state.setChatProject(target, {
            ...creating.layout,
            activeTabId: owned,
          });
          state = useWorkbenchStore.getState();
          if (state.activeTabId !== owned) return;
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
        } else if (owned) {
          const owner = state.captureChatOwnership(owned);
          if (owner) state.setChatComposer(owner, creating.text);
        }
      }}
    />
  );
}
