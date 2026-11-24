import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../app/store";
import type { AgentRailChat } from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { resetBrowserDesktopForTests } from "../services/browserDesktop";
import { ProjectChatNavigation } from "./ProjectChatNavigation";

const initial = useWorkbenchStore.getState();
const store = () => useWorkbenchStore.getState();
type Row = { project: string; chat: AgentRailChat };
let real: Row;
let first: Row;
let second: Row;

async function privateChat(): Promise<Row> {
  await store().createChat(null);
  const chat = store().tabs.find((tab) => tab.id === store().activeTabId)!;
  const row = { project: store().project!.id, chat };
  await store().switchProject(real.project);
  return row;
}

/** Holds the model discovery that keeps a Project switch transitioning. */
function holdDiscovery() {
  const discover = store().loadHarnessModelsForProjection;
  let release!: () => void;
  let entered!: () => void;
  const gate = new Promise<void>((resolve) => (release = resolve));
  const started = new Promise<void>((resolve) => (entered = resolve));
  useWorkbenchStore.setState({
    loadHarnessModelsForProjection: async (...args) => {
      entered();
      await gate;
      return discover(...args);
    },
  });
  return { started, release };
}

function Rail({ rows }: { rows: Row[] }) {
  const current = useWorkbenchStore((state) => state.project?.id);
  return rows.map((row) => (
    <ProjectChatNavigation
      key={row.project}
      projectId={row.project}
      chats={[row.chat]}
      selected={row.project === current}
      hideArchived
    />
  ));
}

function click(view: ReturnType<typeof render>, row: Row) {
  fireEvent.click(
    view.container.querySelector(`button[data-chat-id="${row.chat.id}"]`)!,
  );
}

async function opened(project: string, chat: string) {
  await vi.waitFor(() =>
    expect(store()).toMatchObject({
      projectTransitioning: false,
      project: { id: project },
      activeTabId: chat,
      error: null,
    }),
  );
}

beforeEach(async () => {
  resetBrowserDesktopForTests();
  useWorkbenchStore.setState(initial, true);
  await store().initialize();
  real = {
    project: store().project!.id,
    chat: store().tabs.find((tab) => tab.kind === "chat")!,
  };
  first = await privateChat();
  second = await privateChat();
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  useWorkbenchStore.setState(initial, true);
});

it("opens a rail chat clicked during a project switch once the switch settles", async () => {
  const view = render(<Rail rows={[first, second]} />);
  const held = holdDiscovery();
  click(view, first);
  await held.started;
  expect(store().projectTransitioning).toBe(true);
  click(view, second);
  await act(async () => {});
  expect(store().project?.id).toBe(first.project);
  expect(store().activeTabId).not.toBe(second.chat.id);
  held.release();
  await opened(second.project, second.chat.id);
});

it("opens only the latest rail chat clicked during a project switch", async () => {
  const switches = vi.spyOn(getWorkbenchPorts().project, "switchProject");
  const view = render(<Rail rows={[real, first, second]} />);
  const held = holdDiscovery();
  click(view, first);
  await held.started;
  click(view, real);
  click(view, second);
  held.release();
  await opened(second.project, second.chat.id);
  expect(
    switches.mock.calls.map(([request]) => request.targetProjectId),
  ).toEqual([first.project, second.project]);
});

it("drops a held rail chat that was archived before the switch settled", async () => {
  const switches = vi.spyOn(getWorkbenchPorts().project, "switchProject");
  const view = render(<Rail rows={[real, first, second]} />);
  let held = holdDiscovery();
  click(view, first);
  await held.started;
  click(view, second);
  held.release();
  await opened(second.project, second.chat.id);

  held = holdDiscovery();
  click(view, real);
  await held.started;
  click(view, first);
  // The settling projection reports the held chat as archived.
  useWorkbenchStore.setState((state) => ({
    unscopedChats: state.unscopedChats.map((item) =>
      item.id === first.project
        ? {
            ...item,
            railChats: [],
            railArchived: [{ id: first.chat.id, title: first.chat.title }],
          }
        : item,
    ),
  }));
  held.release();
  await opened(real.project, real.chat.id);
  await act(async () => {});
  expect(store()).toMatchObject({
    project: { id: real.project },
    activeTabId: real.chat.id,
  });
  expect(
    switches.mock.calls.map(([request]) => request.targetProjectId),
  ).toEqual([first.project, second.project, real.project]);
});
