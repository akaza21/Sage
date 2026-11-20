import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it } from "vitest";
import { useWorkbenchStore } from "../app/store";
import { resetBrowserDesktopForTests } from "../services/browserDesktop";
import { openInWorkbench } from "./agentChatHistory";
import { AgentRailArchiveNotice } from "./AgentRailArchiveNotice";
import { setAgentRailArchiveUndo } from "./agentRailInteractions";
import { AgentArchivedSettings } from "../features/settings/AgentArchivedSettings";

const initial = useWorkbenchStore.getState();
let publicProject: string;
beforeEach(async () => {
  resetBrowserDesktopForTests();
  useWorkbenchStore.setState(initial, true);
  await useWorkbenchStore.getState().initialize();
  publicProject = useWorkbenchStore.getState().project!.id;
});
afterEach(() => {
  cleanup();
  setAgentRailArchiveUndo(null);
  useWorkbenchStore.setState(initial, true);
});

async function privateChat(archived = false) {
  await useWorkbenchStore.getState().createChat(null);
  const state = useWorkbenchStore.getState();
  const place = { projectId: state.project!.id, chatId: state.activeTabId! };
  if (archived)
    expect(await state.archiveChat(place.chatId, place.projectId)).toBe(true);
  await useWorkbenchStore.getState().switchProject(publicProject);
  return place;
}

it("history can return to an inactive private chat", async () => {
  const place = await privateChat();
  expect(await openInWorkbench(place)).toEqual({ kind: "opened" });
  expect(useWorkbenchStore.getState().project?.id).toBe(place.projectId);
  expect(useWorkbenchStore.getState().activeTabId).toBe(place.chatId);
});

it("pins an inactive private chat and retains the pin when reopened", async () => {
  const place = await privateChat();
  await useWorkbenchStore.getState().pinChat(place.chatId, place.projectId);
  expect(
    useWorkbenchStore
      .getState()
      .unscopedChats[0].railChats?.find((chat) => chat.id === place.chatId)
      ?.pinned,
  ).toBe(true);
  await openInWorkbench(place);
  expect(
    useWorkbenchStore.getState().tabs.find((tab) => tab.id === place.chatId)
      ?.pinned,
  ).toBe(true);
});

it("keeps archive Undo available after leaving a private chat", async () => {
  const place = await privateChat(true);
  setAgentRailArchiveUndo({ ...place, title: "Private chat" });
  render(<AgentRailArchiveNotice />);
  fireEvent.click(screen.getByRole("button", { name: "Undo archive" }));
  await waitFor(() =>
    expect(useWorkbenchStore.getState().unscopedChats[0]?.railArchived).toEqual(
      [],
    ),
  );
  expect(
    useWorkbenchStore
      .getState()
      .unscopedChats[0].railChats?.some((chat) => chat.id === place.chatId),
  ).toBe(true);
});

it("restores inactive private chats from the aggregate archived settings scope", async () => {
  const place = await privateChat(true);
  render(<AgentArchivedSettings />);
  fireEvent.change(
    screen.getByRole("combobox", { name: "Archived chats project" }),
    { target: { value: "scope:private" } },
  );
  const restore = await screen.findByRole("button", { name: /^Unarchive / });
  await act(async () => fireEvent.click(restore));
  await waitFor(() =>
    expect(useWorkbenchStore.getState().unscopedChats[0]?.railArchived).toEqual(
      [],
    ),
  );
  expect(
    useWorkbenchStore
      .getState()
      .unscopedChats[0].railChats?.some((chat) => chat.id === place.chatId),
  ).toBe(true);
  expect(useWorkbenchStore.getState().project?.id).toBe(publicProject);
});
