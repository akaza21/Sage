import { acpProjectionStoreFor } from "../../../src/app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../../src/app/store";
import type {
  AcpChatPage,
  SageTurnId,
  SageItemId,
  ProjectionRevision,
} from "../../../src/domain/acp";
import { acpDesktop } from "../../../src/services/acpDesktop";
import { installTranscriptSelectionFixture } from "./transcriptSelectionFixtures";

/**
 * The native command boundary returns a rejection once, then commits the
 * suffix. `undo` offers any-turn rewind plus Undo on the latest turn, which
 * the boundary accepts, and `forkLatest` adds a fork through the latest turn
 * only; while `window.__failRewindPreview` is set, a preview rejects with
 * `PREVIEW_FAILURE`.
 */
export const PREVIEW_FAILURE = "Preview check failed.";
export function installConversationRewindFixture(
  inPlace = true,
  residency: "hot" | "cold" = "hot",
  availability: "all" | "latestTurn" | "undo" | "forkLatest" = "all",
  files?: { files: string[]; conflicts: string[]; unverified?: string[] },
) {
  installTranscriptSelectionFixture(false, true);
  const chat = useWorkbenchStore.getState().activeTabId!;
  const projection = acpProjectionStoreFor(chat);
  let page = structuredClone(projection.getState().page!) as AcpChatPage;
  const original = page.turns[0]!;
  const user = original.items.find(
    (item) => item.kind === "text" && item.payload.role === "user",
  );
  if (!user || user.kind !== "text") throw new Error("Missing prompt fixture");
  const first = {
    ...original,
    conversationTarget: true,
    id: "turn-00000000-0000-4000-8000-000000000001" as SageTurnId,
    ordinal: 0,
    terminalAt: 1,
    items: original.items.map((item) => ({
      ...item,
      turnId: "turn-00000000-0000-4000-8000-000000000001" as SageTurnId,
    })),
  };
  const second = {
    ...first,
    id: "turn-00000000-0000-4000-8000-000000000002" as SageTurnId,
    ordinal: 1,
    createdAt: first.createdAt + 1000,
    items: first.items.map((item) => ({
      ...item,
      id: `second-${item.id}` as SageItemId,
      turnId: "turn-00000000-0000-4000-8000-000000000002" as SageTurnId,
    })),
  };
  page = {
    ...page,
    turns: [first, second],
    // Both turns stay conversation targets; a latest-turn offer acts only at
    // its undo turn, so the earlier prompt must show no trigger.
    sessionSurface:
      availability === "all"
        ? page.sessionSurface
        : {
            activities: [],
            undoTurn: second.id,
            // The native side offers the file preview with the undo.
            ...(files && { undoPreviewsFiles: true }),
          },
    sessions: page.sessions.map((session) => ({
      ...session,
      residency,
      capabilities: {
        ...session.capabilities,
        rewind: {
          effects: { conversation: inPlace, files: !!files },
          targets: availability === "latestTurn" ? "latestTurn" : "anyTurn",
          forkAt: true,
          onResume: false,
          forkLatest: availability === "forkLatest",
        },
      },
    })),
  };
  const publish = () => {
    const scope = projection.getState().scope;
    if (scope)
      projection
        .getState()
        .applyAuthoritativePage(scope, structuredClone(page));
  };
  publish();
  acpDesktop.getChatPage = async () => structuredClone(page);
  let refused = !!files;
  const sent: unknown[] = [];
  (window as { __rewindControls?: unknown[] }).__rewindControls = sent;
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    configurable: true,
    value: {
      transformCallback: () => 0,
      invoke: async (
        command: string,
        args: {
          request?: { control?: { kind: string; before: string } };
        },
      ) => {
        if (command !== "control_acp_session") return null;
        const control = args.request?.control;
        sent.push(control);
        if (
          control?.kind === "previewRewind" &&
          (window as { __failRewindPreview?: boolean }).__failRewindPreview
        )
          throw { kind: "rejected", message: PREVIEW_FAILURE };
        if (control?.kind === "undoTurn") return {};
        if (control?.kind === "previewRewind")
          return files
            ? {
                // The native side's view: kept files with why, else the
                // files it would restore and what rewind does not undo.
                preview: ((
                  kept = [
                    [
                      "Files changed since the agent last wrote them, so they stay:",
                      files.conflicts,
                    ] as const,
                    [
                      "Sage can't verify these files in this chat, so they stay as they are:",
                      files.unverified ?? [],
                    ] as const,
                  ].filter(([, paths]) => paths.length),
                ) => ({
                  fileCount: files.files.length,
                  sections: kept.length
                    ? kept.map(([note, paths]) => ({
                        note,
                        paths,
                        count: paths.length,
                        keeps: true,
                      }))
                    : [
                        {
                          note: "These files go back to before this message:",
                          paths: files.files,
                          count: files.files.length,
                        },
                      ],
                  caveat: kept.length
                    ? null
                    : "Rewind does not undo shell commands, commits, branch switches, or files changed outside checkpoints.",
                }))(),
              }
            : {};
        if (control?.kind !== "rewindConversation")
          throw new Error("Unexpected control");
        if (!refused) {
          refused = true;
          throw { kind: "rejected", message: "Harness declined rewind." };
        }
        const target = page.turns.find((turn) => turn.id === control.before);
        if (!target) throw new Error("Unknown rewind target");
        page = {
          ...page,
          turns: page.turns.filter((turn) => turn.ordinal < target.ordinal),
          projectionRevision: String(
            Number(page.projectionRevision) + 1,
          ) as ProjectionRevision,
        };
        publish();
        return {};
      },
    },
  });
  return user.payload.text;
}
