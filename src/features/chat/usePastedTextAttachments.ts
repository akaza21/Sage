import { useEffect, useMemo, useRef, useState, type RefObject } from "react";

import { isTauriRuntime } from "../../ports/nativeInvoke";
import type { PasteAttachability } from "./composerPaste";

/** One staged resource. Only a paste made here keeps its text for preview. */
export interface StagedPromptItem {
  id: string;
  name: string;
  bytes: number;
  /** The pasted text, bounded by the native batch budget; null for files. */
  text: string | null;
  lines: number | null;
}

export interface PastedTextState {
  revision: string | null;
  items: readonly StagedPromptItem[];
  busy: boolean;
}

/** Owner of one chat's staged batch; the lazy flow module drives it. */
export interface StagedOwner {
  chatId: string | undefined;
  ref: RefObject<PastedTextState>;
  set(next: PastedTextState): void;
}

export interface StagedPrompt extends StagedOwner {
  state: PastedTextState;
}

export interface PastedTextAttachments extends StagedPrompt {
  attachability: PasteAttachability;
}

const EMPTY: PastedTextState = { revision: null, items: [], busy: false };
/** Staging, IPC and messages load only where a batch can exist. */
export const loadPasteFlow = () => import("./pastedTextFlow");

/**
 * Pasted text too long for the composer, staged natively for the chat's next
 * send. Rust owns the bytes and consumes them on dispatch; this keeps the
 * chips and, for pastes made here, their text for preview.
 */
export function usePastedTextAttachments(
  chatId: string | undefined,
  /** Changes when a send lands, so a consumed batch is reconciled. */
  turnKey: string | null,
): StagedPrompt {
  const [state, setState] = useState(EMPTY);
  const ref = useRef(state);
  const owner = useMemo<StagedOwner>(
    () => ({
      chatId,
      ref,
      set(next) {
        ref.current = next;
        setState(next);
      },
    }),
    [chatId],
  );
  useEffect(() => {
    if (isTauriRuntime())
      void loadPasteFlow().then((flow) => flow.reconcile(owner));
  }, [owner, turnKey]);
  return { ...owner, state };
}
