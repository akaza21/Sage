import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../../../app/store";
import type {
  AcpChatPage,
  RuntimeEpoch,
  TurnProjection,
} from "../../../domain/acp";
import {
  agentSessionFixture,
  capabilitySetFixture,
} from "../../../domain/acpProjection.fixture";
import {
  acpBackgroundTerminals,
  type BackgroundTerminal,
} from "../../../services/acpBackgroundTerminals";
import { TurnEditSummary } from "./TurnEditSummary";

const server: BackgroundTerminal = {
  handle: "7",
  command: "npm run dev",
  cwd: {
    kind: "resource",
    resource: {
      rootId: "root-1" as never,
      relativePath: "web",
    },
  },
};
const watcher: BackgroundTerminal = {
  handle: "8",
  command: "tsc --watch",
  cwd: { kind: "unavailable", reason: "foreign", label: "outside" },
};

function page(
  supported: boolean,
  latest: TurnProjection["state"] | null = null,
): AcpChatPage {
  const session = agentSessionFixture({
    runtimeEpoch: "1" as RuntimeEpoch,
    capabilities: capabilitySetFixture({
      nativeBackgroundTerminals: supported,
    }),
  });
  return {
    projectId: "project-1",
    chatId: "chat-1",
    title: "Chat",
    projectionRevision: "1",
    currentSessionId: session.id,
    sessions: [session],
    turns: latest
      ? [{ id: "turn-1", state: latest, items: [], activities: [] }]
      : [],
    nextCursor: null,
  } as unknown as AcpChatPage;
}

const labels = { "root-1": "Sage" };
const Dock = ({ page }: { page: AcpChatPage }) => (
  <TurnEditSummary
    page={page}
    rootLabels={labels}
    countIncomplete={false}
    workspaceChanges={[]}
  />
);

beforeEach(() => useWorkbenchStore.setState({ nativeTransitionEpoch: 4 }));
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

it("lists nothing for a session without background terminals", () => {
  const list = vi.spyOn(acpBackgroundTerminals, "list");
  const { container } = render(<Dock page={page(false)} />);
  expect(list).not.toHaveBeenCalled();
  expect(container).toBeEmptyDOMElement();
});

it("lists on mount and when the latest turn ends, and stops one or all", async () => {
  const list = vi
    .spyOn(acpBackgroundTerminals, "list")
    .mockResolvedValue([server, watcher]);
  const stop = vi
    .spyOn(acpBackgroundTerminals, "stop")
    .mockResolvedValueOnce([watcher])
    .mockResolvedValueOnce([]);
  const { container, rerender } = render(<Dock page={page(true)} />);
  expect(
    await screen.findByText("Background terminals (2)"),
  ).toBeInTheDocument();
  expect(list).toHaveBeenCalledWith({
    projectId: "project-1",
    projectGeneration: "4",
    chatId: "chat-1",
    sessionId: "session-a",
    runtimeEpoch: "1",
  });
  expect(screen.getByText("Sage/web")).toBeInTheDocument();
  expect(screen.getByText("outside")).toBeInTheDocument();

  // A running turn does not list again; its end does.
  rerender(<Dock page={page(true, "running")} />);
  expect(list).toHaveBeenCalledTimes(1);
  rerender(<Dock page={page(true, "completed")} />);
  await waitFor(() => expect(list).toHaveBeenCalledTimes(2));

  fireEvent.click(screen.getByRole("button", { name: "Stop npm run dev" }));
  expect(stop).toHaveBeenLastCalledWith(list.mock.calls[0][0], "7");
  expect(
    await screen.findByText("Background terminals (1)"),
  ).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Stop all" }));
  expect(stop).toHaveBeenLastCalledWith(list.mock.calls[0][0], null);
  await waitFor(() => expect(container).toBeEmptyDOMElement());
});
