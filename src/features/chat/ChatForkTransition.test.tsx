import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { settleLazyModules } from "../../test/settleLazyModules";
import { page } from "./acp/acpProjection.fixture";
import { ChatTranscript } from "./ChatTranscript";

const initial = useWorkbenchStore.getState();
const previousFork = getWorkbenchPorts().chat.forkChat;

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initial, true);
  getWorkbenchPorts().chat.forkChat = previousFork;
  vi.restoreAllMocks();
});

it("keeps a refused fork visible when the project transition clears its answer", async () => {
  useWorkbenchStore.setState({
    project: { id: "project-1", name: "Project", roots: [] } as never,
    tabs: [{ id: "chat-1", kind: "chat", title: "Chat", position: 0 }] as never,
    activeTabId: "chat-1",
    projectTransitioning: false,
  });
  const projection = page("completed");
  projection.sessions = [
    {
      id: "session-1",
      runtimeEpoch: "2",
      residency: "hot",
      capabilities: { nativeSessionFork: true },
    },
  ] as never;
  let refuse!: (reason: Error) => void;
  const fork = vi.fn(
    () =>
      new Promise<never>((_, reject) => {
        refuse = reject;
      }),
  );
  getWorkbenchPorts().chat.forkChat = fork;
  function Transcript() {
    const transitioning = useWorkbenchStore(
      (state) => state.projectTransitioning,
    );
    return (
      <ChatTranscript
        transcriptRef={{ current: null }}
        contentRef={{ current: null }}
        endRef={{ current: null }}
        onScroll={() => {}}
        projectTransitioning={transitioning}
        useNativeAcp
        page={transitioning ? null : projection}
        focused
        projectRoots={[]}
        retentionIncomplete={false}
        onOpenResource={() => {}}
        hasEarlier={false}
        loadingEarlier={false}
        historyError={null}
        onLoadEarlier={() => {}}
        messages={[]}
        developmentSimulator={false}
        onRetry={() => {}}
        showJumpToLatest={false}
        onJumpToLatest={() => {}}
      />
    );
  }
  render(<Transcript />);
  await settleLazyModules();
  fireEvent.click(screen.getByRole("button", { name: "More actions" }));
  fireEvent.click(
    await screen.findByRole("menuitem", { name: "Branch in new chat" }),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: /Fork in this workspace/ }),
  );
  expect(fork).toHaveBeenCalledOnce();
  expect(useWorkbenchStore.getState().projectTransitioning).toBe(true);
  expect(
    screen.getByRole("dialog", { name: "Fork chat from here" }),
  ).toBeVisible();
  expect(
    screen.getByRole("button", { name: /Fork in this workspace/ }),
  ).toBeDisabled();
  await act(async () =>
    refuse(new Error("The harness declined to fork this chat.")),
  );
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Couldn't fork this chat. The harness declined to fork this chat.",
  );
  expect(
    screen.getByRole("button", { name: /Fork in this workspace/ }),
  ).toBeEnabled();
  expect(useWorkbenchStore.getState().activeTabId).toBe("chat-1");
  expect(useWorkbenchStore.getState().projectTransitioning).toBe(false);
  await act(async () => {
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  });
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(screen.getByRole("button", { name: "More actions" })).toHaveFocus();
});
