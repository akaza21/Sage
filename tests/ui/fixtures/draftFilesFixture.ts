import { acpDesktop } from "../../../src/services/acpDesktop";

const MAX_CAPTURED = 16;

type Owner = { draftToken: string } | { chatId: string };
type Batch = {
  revision: string;
  files: { id: string; name: string; bytes: number }[];
};

/**
 * Over the draft-plus fixture: answers the native file staging commands as
 * Rust does, one batch per owner keyed by a draft token or a chat id, with
 * whole-batch handoffs and dispatch consuming the batch a turn names. Native
 * picks always pick `notes.txt`. Handoffs and releases are captured on
 * `window.draftFiles`.
 */
export function installDraftFilesFixture() {
  const internals = (
    window as never as {
      __TAURI_INTERNALS__: {
        invoke: (command: string, args: never) => Promise<unknown>;
      };
    }
  ).__TAURI_INTERNALS__;
  const batches = new Map<string, Batch>();
  let handed: { draft: string; chat: string; revision: string } | null = null;
  const control = {
    handOffs: [] as { from: string; to: string }[],
    released: 0,
  };
  Object.assign(window, { draftFiles: control });
  const key = (owner: Owner) =>
    "draftToken" in owner
      ? `draft:${owner.draftToken}`
      : `chat:${owner.chatId}`;
  const bounded = (list: unknown[]) => {
    if (list.length >= MAX_CAPTURED) throw new Error("Capture limit reached");
  };
  const answer = (command: string, args: Record<string, never>) => {
    if (command === "list_prompt_files")
      return batches.get(key(args.owner)) ?? null;
    if (command === "release_draft_prompt_files") {
      control.released += 1;
      return void batches.delete(key(args.draft));
    }
    const request = args.request as Record<string, never>;
    if (command === "stage_prompt_files") {
      const owner = key(request.owner);
      if ((batches.get(owner)?.revision ?? null) !== request.expectedRevision)
        throw "Attachments changed; reload the staged attachments before retrying.";
      // One draft slot at a time, as native sweeps a leftover one.
      for (const other of [...batches.keys()])
        if (other.startsWith("draft:") && other !== owner)
          batches.delete(other);
      const files = batches.get(owner)?.files ?? [];
      const next = {
        revision: crypto.randomUUID(),
        files: [
          ...files,
          { id: crypto.randomUUID(), name: "notes.txt", bytes: 2048 },
        ],
      };
      batches.set(owner, next);
      return next;
    }
    if (command === "hand_off_prompt_files") {
      const [from, to] = [key(request.from), key(request.to)];
      // As native: a draft to a chat, or back only to undo that exact move.
      const admitted = from.startsWith("draft:")
        ? to.startsWith("chat:")
        : to === handed?.draft &&
          from === handed.chat &&
          request.revision === handed.revision;
      if (!admitted)
        throw "Attachments move only from a draft to its chat, and back.";
      const batch = batches.get(from);
      if (batch?.revision !== request.revision || batches.has(to))
        throw "Attachments changed; review them and send again.";
      bounded(control.handOffs);
      control.handOffs.push({ from: from.split(":")[0], to: to.split(":")[0] });
      batches.delete(from);
      const moved = { ...batch, revision: crypto.randomUUID() };
      batches.set(to, moved);
      handed = from.startsWith("draft:")
        ? { draft: from, chat: to, revision: moved.revision }
        : null;
      return moved;
    }
    return undefined;
  };
  const invoke = internals.invoke;
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    configurable: true,
    value: {
      ...internals,
      invoke: async (command: string, args: never) =>
        command.includes("prompt_file")
          ? structuredClone(answer(command, args))
          : invoke(command, args),
    },
  });
  // Dispatch consumes the batch the turn names, so it is sent once.
  const sendTurn = acpDesktop.sendTurn;
  acpDesktop.sendTurn = async (request) => {
    const owner = `chat:${request.chatId}`;
    const staged = "staged" in request ? request.staged : undefined;
    if (
      staged &&
      "revision" in staged &&
      batches.get(owner)?.revision === staged.revision
    )
      batches.delete(owner);
    return sendTurn(request);
  };
}
