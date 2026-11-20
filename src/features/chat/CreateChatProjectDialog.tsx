import type { NewChatDraft } from "../../app/newChatDraftSlice";
import type { ProjectLayoutRequest } from "../../domain/workbench";
import { lazyParts } from "./lazyParts";
import type * as CreationChunk from "./ChatProjectCreationSurface";
import type { ChatComposerDraft } from "../../domain/chatOwnership";

export interface ChatProjectCreation {
  layout: ProjectLayoutRequest | null;
  draft: NewChatDraft | null;
  text: string;
  composer?: ChatComposerDraft;
}

const CreationPart = lazyParts<typeof CreationChunk>(
  () => import("./ChatProjectCreationSurface"),
);
export function CreateChatProjectDialog({
  creating,
  onClose,
}: {
  creating: ChatProjectCreation;
  onClose: () => void;
}) {
  return (
    <CreationPart
      part="ChatProjectCreationSurface"
      creating={creating}
      onClose={onClose}
    />
  );
}
