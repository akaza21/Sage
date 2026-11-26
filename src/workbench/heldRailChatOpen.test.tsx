import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../app/store";
import type { AgentRailChat } from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { resetBrowserDesktopForTests } from "../services/browserDesktop";
import { ProjectChatNavigation } from "./ProjectChatNavigation";
import { useNotificationNavigation } from "./useTurnNotifications";

// A native notification click: the shell queues the chat, then emits a wake.
const native = vi.hoisted(() => ({
  tauri: false,
  clicked: [] as { projectId: string; chatId: string }[],
  wake: () => {},
}));
vi.mock("../ports/nativeInvoke", async (actual) => ({
  ...(await actual<object>()),
  isTauriRuntime: () => native.tauri,
}));
vi.mock("../ports/nativeNotificationPort", () => ({
  nativeNotificationPort: {
    takeChat: async () => native.clicked.shift() ?? null,
    listenOpen: async (wake: () => void) => {
      native.wake = wake;
      return () => {};
    },
  },
}));

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
  return (
    <>
      {/* Stands in for a rail project row, which switches project. */}
      <button
        type="button"
        onClick={() => void store().switchProject(real.project)}
      >
        Open project
      </button>
      {rows.map((row) => (
        <ProjectChatNavigation
          key={row.project}
          projectId={row.project}
          chats={[row.chat]}
          selected={row.project === current}
          hideArchived
        />
      ))}
    </>
  );
}

async function idle() {
  await vi.waitFor(() => expect(store().projectTransitioning).toBe(false));
  for (let turn = 0; turn < 3; turn++)
    await act(() => new Promise((resolve) => setTimeout(resolve, 0)));
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
  native.tauri = false;
  native.clicked = [];
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

it("cancels a held rail chat on a later rail action or when the rail unmounts", async () => {
  const switches = vi.spyOn(getWorkbenchPorts().project, "switchProject");
  const view = render(<Rail rows={[real, first, second]} />);
  let held = holdDiscovery();
  click(view, first);
  await held.started;
  click(view, second);
  // A project row clicked after the held chat is the user's latest action.
  fireEvent.click(view.getByRole("button", { name: "Open project" }));
  held.release();
  await opened(first.project, first.chat.id);
  await idle();
  expect(store().project?.id).toBe(first.project);

  held = holdDiscovery();
  click(view, second);
  await held.started;
  click(view, first);
  view.unmount();
  held.release();
  await opened(second.project, second.chat.id);
  await idle();
  expect(store().project?.id).toBe(second.project);
  expect(
    switches.mock.calls.map(([request]) => request.targetProjectId),
  ).toEqual([first.project, second.project]);
});

it("keeps a failed switch's error instead of replaying a held rail chat", async () => {
  const ports = getWorkbenchPorts().project;
  let fail!: () => void;
  const failing = new Promise<void>((resolve) => (fail = resolve));
  const switches = vi
    .spyOn(ports, "switchProject")
    .mockImplementationOnce(async () => {
      await failing;
      throw new Error("Switch failed");
    });
  const view = render(<Rail rows={[first, second]} />);
  click(view, first);
  await vi.waitFor(() => expect(switches).toHaveBeenCalledTimes(1));
  click(view, second);
  fail();
  await vi.waitFor(() => expect(store().error).toBe("Switch failed"));
  await idle();
  expect(store()).toMatchObject({
    project: { id: real.project },
    activeTabId: real.chat.id,
    error: "Switch failed",
  });
  expect(switches).toHaveBeenCalledTimes(1);
});

function Notifications() {
  useNotificationNavigation(() => {});
  return null;
}

/** Mounts the rail with the native notification listener of a desktop shell. */
async function desktopRail(rows: Row[]) {
  native.tauri = true;
  const view = render(
    <>
      <Rail rows={rows} />
      <Notifications />
    </>,
  );
  await idle();
  return view;
}

async function notify(row: Row) {
  native.clicked.push({ projectId: row.project, chatId: row.chat.id });
  native.wake();
  // Lets the click reach the held-open slot before the test moves on.
  await act(() => new Promise((resolve) => setTimeout(resolve, 0)));
}

it("opens only a notification clicked after a held rail chat, as the latest action", async () => {
  const switches = vi.spyOn(getWorkbenchPorts().project, "switchProject");
  const view = await desktopRail([first, second]);
  const held = holdDiscovery();
  void store().switchProject(first.project);
  await held.started;
  click(view, second);
  await notify(first);
  held.release();
  await opened(first.project, first.chat.id);
  await idle();
  expect(store()).toMatchObject({
    project: { id: first.project },
    activeTabId: first.chat.id,
    error: null,
  });
  // The replaced rail click never switches to its project.
  expect(
    switches.mock.calls.map(([request]) => request.targetProjectId),
  ).toEqual([first.project]);
});

it("drops a notification clicked during a switch once a project row is clicked", async () => {
  const switches = vi.spyOn(getWorkbenchPorts().project, "switchProject");
  const view = await desktopRail([first, second]);
  const held = holdDiscovery();
  click(view, first);
  await held.started;
  await notify(second);
  fireEvent.click(view.getByRole("button", { name: "Open project" }));
  held.release();
  await opened(first.project, first.chat.id);
  await idle();
  expect(store().project?.id).toBe(first.project);
  expect(
    switches.mock.calls.map(([request]) => request.targetProjectId),
  ).toEqual([first.project]);
});
