import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../app/store";
import { clearAcpProjectionRegistry } from "../app/acpProjectionRegistry";
import type { WorkbenchTab } from "../domain/workbench";
import { AgentTaskHeader } from "./AgentTaskHeader";
import { AgentTaskResourcesPart } from "./agentTaskHeaderLazy";

const initialState = useWorkbenchStore.getState();
const tab: WorkbenchTab = {
  id: "chat:lazy",
  kind: "chat",
  title: "Lazy header",
  position: 0,
  pinned: false,
  dirty: false,
};

beforeEach(() => {
  useWorkbenchStore.setState({
    ...initialState,
    project: { id: "project-lazy", name: "Sage", roots: [] },
    tabs: [tab],
    activeTabId: tab.id,
  });
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  clearAcpProjectionRegistry();
  useWorkbenchStore.setState(initialState, true);
});

function mountHeader() {
  return render(<AgentTaskHeader tab={tab} chatCount={1} snapshot={null} />);
}

it("keeps the header painted during a cold open without a loading flash", async () => {
  const preload = vi.spyOn(AgentTaskResourcesPart, "preload");
  mountHeader();
  expect(preload).not.toHaveBeenCalled();
  expect(screen.queryByRole("dialog", { name: "Chat resources" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Show chat resources" }));
  expect(screen.getByText("Lazy header")).toBeVisible();
  expect(screen.queryByText(/Loading/)).toBeNull();
  expect(
    await screen.findByRole("dialog", { name: "Chat resources" }),
  ).toBeVisible();
});

it("prewarms after first paint without mounting the panel, then opens synchronously", async () => {
  vi.useFakeTimers();
  const preload = vi.spyOn(AgentTaskResourcesPart, "preload");
  mountHeader();
  act(() => vi.advanceTimersByTime(999));
  expect(preload).not.toHaveBeenCalled();
  await act(async () => {
    vi.advanceTimersByTime(1);
    await preload.mock.results[0]!.value;
  });
  expect(preload).toHaveBeenCalledOnce();
  expect(screen.queryByRole("dialog", { name: "Chat resources" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Show chat resources" }));
  expect(screen.getByRole("dialog", { name: "Chat resources" })).toBeVisible();
});

it("cancels idle prewarming when the header unmounts", () => {
  vi.useFakeTimers();
  const preload = vi.spyOn(AgentTaskResourcesPart, "preload");
  const mounted = mountHeader();
  mounted.unmount();
  act(() => vi.advanceTimersByTime(1000));
  expect(preload).not.toHaveBeenCalled();
});

it("prewarms on pointer intent and keeps inline rename focused and authoritative", async () => {
  const renameChat = vi
    .fn()
    .mockResolvedValueOnce(false)
    .mockResolvedValueOnce(true);
  useWorkbenchStore.setState({ renameChat, error: "Rename refused" });
  const preload = vi.spyOn(AgentTaskResourcesPart, "preload");
  mountHeader();
  const trigger = screen.getByRole("button", { name: "Rename chat" });
  await act(async () => {
    fireEvent.pointerEnter(trigger);
    await preload.mock.results[0]!.value;
  });
  fireEvent.click(trigger);
  const input = screen.getByRole("textbox", { name: "New chat name" });
  expect(input).toHaveFocus();
  fireEvent.change(input, { target: { value: "Renamed" } });
  fireEvent.click(screen.getByRole("button", { name: "Save chat name" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Rename refused");
  expect(renameChat).toHaveBeenLastCalledWith(
    tab.id,
    "Renamed",
    "project-lazy",
  );
  fireEvent.click(screen.getByRole("button", { name: "Save chat name" }));
  await act(async () => undefined);
  expect(screen.queryByRole("textbox", { name: "New chat name" })).toBeNull();
});

it("keeps header mutations scoped and refreshes the open move destinations", () => {
  const pinChat = vi.fn(async () => undefined);
  const moveChat = vi.fn(async () => undefined);
  const archiveChat = vi.fn(async () => true);
  useWorkbenchStore.setState({ pinChat, moveChat, archiveChat, projects: [] });
  render(<AgentTaskHeader tab={tab} chatCount={2} snapshot={null} />);
  const openActions = () => {
    fireEvent.click(
      screen.getByRole("button", { name: "Chat actions for Lazy header task" }),
    );
    fireEvent.click(
      screen.getByRole("menuitem", { name: "More chat actions in Sage" }),
    );
  };

  openActions();
  fireEvent.click(screen.getByRole("menuitem", { name: "Pin chat" }));
  expect(pinChat).toHaveBeenCalledWith(tab.id, "project-lazy");
  openActions();
  fireEvent.click(screen.getByRole("menuitem", { name: "Move to project" }));
  expect(screen.getByText("No other project available.")).toBeVisible();
  act(() =>
    useWorkbenchStore.setState({
      projects: [
        { id: "destination", name: "Work", rootCount: 1, lastOpenedAt: 1 },
      ],
    }),
  );
  fireEvent.click(screen.getByRole("menuitem", { name: "Move chat to Work" }));
  expect(moveChat).toHaveBeenCalledWith(tab.id, "destination", "project-lazy");
  openActions();
  fireEvent.click(screen.getByRole("menuitem", { name: "Archive chat" }));
  expect(archiveChat).toHaveBeenCalledWith(tab.id, "project-lazy");
});
