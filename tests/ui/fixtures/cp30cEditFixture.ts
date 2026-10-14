import { acpProjectionStoreFor } from "../../../src/app/acpProjectionRegistry";
import type { AcpItem, SageChatId } from "../../../src/domain/acp";

export function installCp30cEdit(
  lifecycle: "streaming" | "completed" | "failed",
  chatId?: string,
) {
  for (const projection of document.querySelectorAll<HTMLElement>(
    ".acp-chat-projection[data-chat-id]",
  )) {
    if (chatId && projection.dataset.chatId !== chatId) continue;
    const store = acpProjectionStoreFor(
      projection.dataset.chatId as SageChatId,
    );
    const page = structuredClone(store.getState().page);
    const turn = page?.turns.at(-1);
    const saved = page?.turns
      .flatMap((candidate) => candidate.items)
      .find((item) => item.kind === "fileChangeDiff");
    if (!page || !turn || !saved || saved.kind !== "fileChangeDiff")
      throw new Error("CP-30c requires a settled changed-file projection");
    turn.state = "running";
    turn.terminalAt = null;
    turn.revision = String(Number(turn.revision) + 1) as typeof turn.revision;
    turn.items = [
      ...turn.items.filter((item) => item.id !== "cp30c-live"),
      {
        ...saved,
        id: "cp30c-live",
        turnId: turn.id,
        ordinal: 99,
        lifecycle,
        terminalAt: lifecycle === "streaming" ? null : saved.updatedAt,
        revision: turn.revision,
        payload: {
          truncated: false,
          changes:
            lifecycle === "completed"
              ? [
                  {
                    ...saved.payload.changes[0],
                    resource: {
                      kind: "resource",
                      resource: {
                        rootId: "visual-root",
                        relativePath: "src/another.ts",
                      },
                    },
                    additions: null,
                    deletions: null,
                    pairState: "unavailable",
                    snapshotRevision: "0",
                    oldBytes: null,
                    newBytes: null,
                    contentDigest: null,
                  },
                ]
              : [],
        },
      } as AcpItem,
    ];
    store.setState({ page });
  }
}

export function installCp30cDisclosure(totals: "unknown" | "partial") {
  for (const projection of document.querySelectorAll<HTMLElement>(
    ".acp-chat-projection[data-chat-id]",
  )) {
    const store = acpProjectionStoreFor(
      projection.dataset.chatId as SageChatId,
    );
    const page = structuredClone(store.getState().page);
    if (!page) throw new Error("Expected settled chat");
    page.turns = page.turns.filter((turn) => turn.id !== "cp30c-archived");
    const turn = page.turns.at(-1)!;
    const saved = page.turns
      .flatMap((entry) => entry.items)
      .find(
        (item) =>
          item.kind === "fileChangeDiff" &&
          item.sessionId === page.currentSessionId,
      );
    if (!saved || saved.kind !== "fileChangeDiff")
      throw new Error("Expected saved edit");
    const original = saved.payload.changes[0];
    turn.revision = String(Number(turn.revision) + 1) as typeof turn.revision;
    turn.items = [
      ...turn.items.filter((item) => item.kind !== "fileChangeDiff"),
      {
        ...saved,
        id: "cp30c-disclosure",
        turnId: turn.id,
        runtimeEpoch: turn.runtimeEpoch,
        revision: turn.revision,
        payload: {
          truncated: false,
          changes: Array.from({ length: 16 }, (_, index) => ({
            ...original,
            resource: {
              kind: "resource",
              resource: {
                rootId: "visual-root",
                relativePath: "src/disclosure-" + index + ".tsx",
              },
            },
            additions: null,
            deletions: totals === "partial" ? 1 : null,
            pairState: "unavailable",
            snapshotRevision: "0",
            contentDigest: null,
            oldBytes: null,
            newBytes: null,
          })),
        },
      } as AcpItem,
    ];
    const sessionId = "cp30c-previous" as typeof turn.sessionId;
    if (!page.sessions.some((session) => session.id === sessionId))
      page.sessions = [
        ...page.sessions,
        { ...page.sessions[0], id: sessionId },
      ];
    page.turns = [
      {
        ...turn,
        id: "cp30c-archived" as typeof turn.id,
        sessionId,
        items: [
          {
            ...saved,
            id: "cp30c-archived-edit",
            turnId: "cp30c-archived",
            sessionId,
            runtimeEpoch: turn.runtimeEpoch,
            payload: {
              ...saved.payload,
              changes: [
                {
                  ...original,
                  resource: {
                    kind: "resource",
                    resource: {
                      rootId: "visual-root",
                      relativePath: "src/archived.ts",
                    },
                  },
                  additions: null,
                  deletions: totals === "partial" ? 1 : null,
                },
              ],
            },
          } as AcpItem,
        ],
      },
      ...page.turns,
    ];
    store.setState({ page });
  }
}
