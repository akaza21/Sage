/** Lazy: staging, its IPC and paste decisions load with ComposerPaste. The
 * chat's one staged batch: pasted text and picked files alike. */
import type { RefObject } from "react";

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
  type PasteAttachability,
} from "./composerPasteText";
import type { HeldPaste, PastedTextState } from "./pastedTextTypes";

type Announce = (message: string) => void;

/** One chat's staged batch as the composer shows it. */
export interface StagedOwner {
  chatId: string | undefined;
  ref: RefObject<PastedTextState>;
  set(next: PastedTextState): void;
  /** Marks the batch as read back from native state. */
  settle(): void;
}

const WEB_ONLY: PasteAttachability = { nativeAcp: false, session: null };

function failure(error: unknown, fallback: string): string {
  if (typeof error === "string") return error;
  return error instanceof Error ? error.message : fallback;
}

/** The chat's own owner; never another tab's, such as the active one. */
function owner(chatId: string | undefined): PromptFileOwner | null {
  const state = useWorkbenchStore.getState();
  const intent = chatId ? state.captureChatOwnership(chatId) : null;
  if (!intent || !isTauriRuntime()) return null;
  return {
    projectId: intent.projectId,
    chatId: intent.chatId,
    projectGeneration: intent.transitionEpoch,
  };
}

/** Native truth replaces the list; the paste just staged keeps its text by
 * the id native names for it, never by guessing which entry is new. */
function apply(
  staged: StagedOwner,
  result: StagedPromptFiles | null,
  added?: { text: string; lines: number },
) {
  const current = staged.ref.current;
  const known = new Map(current.items.map((item) => [item.id, item]));
  staged.set({
    busy: current.busy,
    revision: result?.revision ?? null,
    items: (result?.files ?? []).map(({ id, name, bytes }) => {
      const kept = known.get(id) ?? (id === result?.added ? added : null);
      return {
        id,
        name,
        bytes,
        text: kept?.text ?? null,
        lines: kept?.lines ?? null,
      };
    }),
  });
}

function setBusy(staged: StagedOwner, busy: boolean) {
  staged.set({ ...staged.ref.current, busy });
}

export async function reconcile(staged: StagedOwner) {
  const target = owner(staged.chatId);
  if (!target) return;
  try {
    apply(staged, await port.listPromptFiles(target));
  } catch {
    // The next send or paste reports any native failure.
  }
}

/** Attaches, or refuses with a message; the composer text is never changed. */
export async function pasteOver(
  staged: StagedOwner,
  attachability: PasteAttachability | undefined,
  { before, value, after }: HeldPaste,
  announce: Announce,
  /** A send is in flight and may consume the batch. */
  sending = false,
) {
  const state = staged.ref.current;
  const decision = pasteDecision({
    before,
    pasted: value,
    after,
    staged: {
      bytes: state.items.reduce((sum, item) => sum + item.bytes, 0),
      count: state.items.length,
    },
    refusal: pasteAttachmentRefusal(
      attachability && isTauriRuntime() ? attachability : WEB_ONLY,
    ),
  });
  if (decision.kind === "refuse") announce(decision.message);
  if (decision.kind !== "attach") return;
  if (sending)
    return announce(
      "Wait for the message to finish sending, then paste again. Nothing was pasted.",
    );
  const target = owner(staged.chatId);
  if (!target) return announce("This chat is not ready for attachments.");
  if (state.busy)
    return announce("Wait for the previous attachment to finish.");
  const lines = lineCount(value);
  const label = pastedTextLabel(decision.bytes, lines);
  announce(`Attaching ${label}…`);
  setBusy(staged, true);
  try {
    const result = await port.stagePromptText({
      owner: target,
      expectedRevision: staged.ref.current.revision,
      text: value,
    });
    apply(staged, result, { text: value, lines });
    announce(`Attached ${label}.`);
  } catch (error) {
    await reconcile(staged);
    announce(
      `${failure(error, "Could not attach the pasted text.")} Nothing was pasted.`,
    );
  } finally {
    setBusy(staged, false);
  }
}

/**
 * Adds the files picked in native's dialog, which stages them in Rust: the
 * WebView never sees a path or the bytes. A cancelled pick keeps the batch.
 */
export async function pickFiles(
  staged: StagedOwner,
  announce: Announce,
  /** A send is in flight and may consume the batch. */
  sending: boolean,
) {
  if (sending)
    return announce(
      "Wait for the message to finish sending, then attach again. Nothing was attached.",
    );
  const target = owner(staged.chatId);
  if (!target) return announce("This chat is not ready for attachments.");
  if (staged.ref.current.busy)
    return announce("Wait for the previous attachment to finish.");
  setBusy(staged, true);
  try {
    const result = await port.stagePromptFiles({
      owner: target,
      pick: "files",
      expectedRevision: staged.ref.current.revision,
    });
    if (!result) return;
    apply(staged, result);
    const count = result.files.length;
    announce(
      `${count} ${count === 1 ? "file" : "files"} attached to the next message.`,
    );
  } catch (error) {
    await reconcile(staged);
    announce(failure(error, "The attachments could not be updated."));
  } finally {
    setBusy(staged, false);
  }
}

export async function removeStaged(
  staged: StagedOwner,
  id: string,
  label: string,
  announce: Announce,
) {
  const target = owner(staged.chatId);
  const revision = staged.ref.current.revision;
  if (!target || !revision) return;
  try {
    apply(staged, await port.removePromptFile({ owner: target, revision, id }));
    announce(`Removed ${label}.`);
  } catch (error) {
    await reconcile(staged);
    announce(failure(error, "Could not remove the attachment."));
  }
}
