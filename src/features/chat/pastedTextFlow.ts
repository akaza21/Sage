/** Lazy: staging and paste decisions load with the first over-cap input. */
import { useWorkbenchStore } from "../../app/store";
import { isTauriRuntime } from "../../ports/nativeInvoke";
import { nativePromptAttachments as port } from "../../ports/nativePromptAttachments";
import type {
  PromptFileOwner,
  StagedPromptFiles,
} from "../../ports/promptAttachments";
import {
  lineCount,
  pasteAttachmentRefusal,
  pastedTextLabel,
  pasteDecision,
} from "./composerPaste";
import type {
  PastedTextAttachments,
  StagedOwner,
} from "./usePastedTextAttachments";

export { typedOverflowMessage } from "./composerPaste";

type Announce = (message: string) => void;
const WEB_ONLY = { nativeAcp: false, session: null, queued: 0 };

function failure(error: unknown, fallback: string): string {
  if (typeof error === "string") return error;
  return error instanceof Error ? error.message : fallback;
}

function owner(chatId: string | undefined): PromptFileOwner | null {
  const state = useWorkbenchStore.getState();
  const id = chatId ?? state.activeTabId ?? undefined;
  const intent = id ? state.captureChatOwnership(id) : null;
  if (!intent || !isTauriRuntime()) return null;
  return {
    projectId: intent.projectId,
    chatId: intent.chatId,
    projectGeneration: intent.transitionEpoch,
  };
}

/** Native truth replaces the list; a new id answers the paste just staged. */
function apply(
  pasted: StagedOwner,
  result: StagedPromptFiles | null,
  added?: { text: string; lines: number },
) {
  const current = pasted.ref.current;
  const known = new Map(current.items.map((item) => [item.id, item]));
  pasted.set({
    busy: current.busy,
    revision: result?.revision ?? null,
    items: (result?.files ?? []).map(({ id, name, bytes }) => {
      const prior = known.get(id);
      const fresh = prior ? undefined : added;
      if (!prior) added = undefined;
      return {
        id,
        name,
        bytes,
        text: prior ? prior.text : (fresh?.text ?? null),
        lines: prior ? prior.lines : (fresh?.lines ?? null),
      };
    }),
  });
}

function setBusy(pasted: StagedOwner, busy: boolean) {
  pasted.set({ ...pasted.ref.current, busy });
}

export async function reconcile(pasted: StagedOwner) {
  const target = owner(pasted.chatId);
  if (!target) return;
  try {
    apply(pasted, await port.listPromptFiles(target));
  } catch {
    // The next send or paste reports any native failure.
  }
}

/** Attaches, or refuses with a message; the composer text is never changed. */
export async function pasteOver(
  pasted: PastedTextAttachments | undefined,
  before: string,
  value: string,
  after: string,
  announce: Announce,
) {
  const state = pasted?.ref.current;
  const decision = pasteDecision({
    before,
    pasted: value,
    after,
    staged: {
      bytes: state?.items.reduce((sum, item) => sum + item.bytes, 0) ?? 0,
      count: state?.items.length ?? 0,
    },
    refusal: pasteAttachmentRefusal(
      pasted && isTauriRuntime() ? pasted.attachability : WEB_ONLY,
    ),
  });
  if (decision.kind === "refuse") announce(decision.message);
  if (decision.kind !== "attach" || !pasted) return;
  const target = owner(pasted.chatId);
  if (!target) return announce("This chat is not ready for attachments.");
  if (pasted.ref.current.busy)
    return announce("Wait for the previous attachment to finish.");
  const lines = lineCount(value);
  const label = pastedTextLabel(decision.bytes, lines);
  announce(`Attaching ${label}…`);
  setBusy(pasted, true);
  try {
    const staged = await port.stagePromptText({
      owner: target,
      expectedRevision: pasted.ref.current.revision,
      text: value,
    });
    apply(pasted, staged, { text: value, lines });
    announce(`Attached ${label}.`);
  } catch (error) {
    await reconcile(pasted);
    announce(
      `${failure(error, "Could not attach the pasted text.")} Nothing was pasted.`,
    );
  } finally {
    setBusy(pasted, false);
  }
}

export async function removeStaged(
  pasted: PastedTextAttachments,
  id: string,
  label: string,
  announce: Announce,
) {
  const target = owner(pasted.chatId);
  const revision = pasted.ref.current.revision;
  if (!target || !revision) return;
  try {
    apply(pasted, await port.removePromptFile({ owner: target, revision, id }));
    announce(`Removed ${label}.`);
  } catch (error) {
    await reconcile(pasted);
    announce(failure(error, "Could not remove the attachment."));
  }
}
