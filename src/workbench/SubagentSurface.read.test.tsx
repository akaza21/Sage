import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type {
  AcpChatPage,
  ProjectionRevision,
  RuntimeEpoch,
  SageAgentSessionId,
} from "../domain/acp";
import { SubagentSurface } from "./SubagentSurface";

const readChild = vi.hoisted(() => vi.fn());
vi.mock("../ports/nativeInvoke", async (importOriginal) => ({
  ...((await importOriginal()) as object),
  invokeNative: readChild,
}));
afterEach(() => {
  cleanup();
  readChild.mockReset();
});

function childPage(): AcpChatPage {
  return {
    projectId: "project",
    chatId: "chat",
    projectionRevision: "7",
    currentSessionId: "session",
    sessions: [
      {
        id: "session",
        runtimeEpoch: "2",
        capabilities: { childTranscriptRead: true },
      },
    ],
    turns: [
      {
        id: "turn",
        sessionId: "session",
        runtimeEpoch: "2",
        items: [
          {
            id: "spawn",
            kind: "collaboration",
            payload: {
              agentKey: "child",
              receiverLabels: [],
              agentStates: [],
            },
          },
        ],
        activities: [],
      },
    ],
  } as unknown as AcpChatPage;
}

const current = {
  kind: "current",
  snapshot: {
    childThreadId: "child",
    parentThreadId: "parent",
    retentionLimited: false,
    turns: [],
  },
};

it.each([
  [
    "stale revision",
    { kind: "conflict", resource: "projectionRevision" },
    "Child transcript read is stale. Reopen this view after the chat updates.",
  ],
  [
    "stale owner",
    { kind: "conflict", resource: "session" },
    "Child transcript read is stale. Reopen this view after the chat updates.",
  ],
  [
    "rejected",
    { kind: "rejected" },
    "Child transcript read was rejected. No transcript was loaded.",
  ],
  [
    "unavailable",
    { kind: "unavailable" },
    "Child transcript is unavailable. Try reopening this view.",
  ],
  [
    "recovery required",
    { kind: "recoveryRequired" },
    "Child transcript read needs recovery. Reload the session.",
  ],
  [
    "unknown",
    "private wire details",
    "Child transcript is unavailable. Try reopening this view.",
  ],
])("shows a bounded truthful %s failure", async (_name, failure, message) => {
  readChild.mockRejectedValue(
    typeof failure === "object"
      ? { ...failure, message: "private wire details" }
      : failure,
  );
  render(
    <SubagentSurface
      page={childPage()}
      projectGeneration="3"
      itemId="spawn"
      agentKey="child"
    />,
  );
  expect(await screen.findByText(message)).toBeVisible();
  expect(screen.queryByText("private wire details")).toBeNull();
  expect(screen.queryByText("Current child transcript")).toBeNull();
  expect(readChild).toHaveBeenCalledTimes(1);
});

it("shows too-large without claiming an empty or partial transcript", async () => {
  readChild.mockResolvedValue({ kind: "tooLarge" });
  render(
    <SubagentSurface
      page={childPage()}
      projectGeneration="3"
      itemId="spawn"
      agentKey="child"
    />,
  );
  expect(
    await screen.findByText("Child transcript is too large to display."),
  ).toBeVisible();
  expect(screen.queryByText("No child activity was captured.")).toBeNull();
});

it.each(["capability", "epoch", "child", "session"])(
  "never dispatches with missing or mismatched %s",
  (guard) => {
    const page = childPage();
    if (guard === "capability")
      page.sessions![0].capabilities!.childTranscriptRead = false;
    if (guard === "epoch") page.sessions![0].runtimeEpoch = "9" as RuntimeEpoch;
    if (guard === "session")
      page.currentSessionId = "other" as SageAgentSessionId;
    render(
      <SubagentSurface
        page={page}
        projectGeneration="3"
        itemId="spawn"
        agentKey={guard === "child" ? "foreign" : "child"}
      />,
    );
    expect(readChild).not.toHaveBeenCalled();
  },
);

it.each(["project", "chat", "generation", "revision", "epoch", "session"])(
  "discards late success and failure after %s changes",
  async (ownerPart) => {
    let finish!: (value: unknown) => void;
    let reject!: (error: unknown) => void;
    readChild.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    readChild.mockImplementationOnce(
      () =>
        new Promise((_resolve, fail) => {
          reject = fail;
        }),
    );
    readChild.mockResolvedValueOnce({ kind: "tooLarge" });
    const page = childPage();
    const view = render(
      <SubagentSurface
        page={page}
        projectGeneration="3"
        itemId="spawn"
        agentKey="child"
      />,
    );
    const changed = childPage();
    if (ownerPart === "project")
      changed.projectId = "other" as typeof changed.projectId;
    if (ownerPart === "chat") changed.chatId = "other" as typeof changed.chatId;
    if (ownerPart === "revision")
      changed.projectionRevision = "8" as ProjectionRevision;
    if (ownerPart === "epoch") {
      changed.sessions![0].runtimeEpoch = "9" as RuntimeEpoch;
      changed.turns[0].runtimeEpoch = "9" as RuntimeEpoch;
    }
    if (ownerPart === "session") {
      changed.currentSessionId = "other" as SageAgentSessionId;
      changed.sessions![0].id = "other" as SageAgentSessionId;
      changed.turns[0].sessionId = "other" as SageAgentSessionId;
    }
    const generation = ownerPart === "generation" ? "4" : "3";
    view.rerender(
      <SubagentSurface
        page={changed}
        projectGeneration={generation}
        itemId="spawn"
        agentKey="child"
      />,
    );
    await act(async () => finish(current));
    expect(screen.queryByText("Current child transcript")).toBeNull();
    view.rerender(
      <SubagentSurface
        page={{ ...changed, projectionRevision: "10" as ProjectionRevision }}
        projectGeneration={generation}
        itemId="spawn"
        agentKey="child"
      />,
    );
    expect(await screen.findByText(/too large to display/)).toBeVisible();
    await act(async () => reject({ kind: "rejected" }));
    expect(screen.queryByText(/read was rejected/)).toBeNull();
    expect(readChild).toHaveBeenCalledTimes(3);
  },
);

it("drops a pending read when the Session is superseded", async () => {
  let finish!: (value: unknown) => void;
  readChild.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const page = childPage();
  const view = render(
    <SubagentSurface
      page={page}
      projectGeneration="3"
      itemId="spawn"
      agentKey="child"
    />,
  );
  view.rerender(
    <SubagentSurface
      page={{
        ...page,
        currentSessionId: "other" as typeof page.currentSessionId,
      }}
      projectGeneration="3"
      itemId="spawn"
      agentKey="child"
    />,
  );
  await act(async () => finish(current));
  await waitFor(() =>
    expect(screen.queryByText("Current child transcript")).toBeNull(),
  );
  expect(
    screen.getByText(/earlier session and isn’t available here/),
  ).toBeVisible();
  expect(readChild).toHaveBeenCalledTimes(1);
});
