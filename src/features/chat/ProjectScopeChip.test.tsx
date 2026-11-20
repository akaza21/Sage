import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it } from "vitest";
import { useWorkbenchStore } from "../../app/store";
import { resetBrowserDesktopForTests } from "../../services/browserDesktop";
import { ProjectScopeChip } from "./ProjectScopeChip";

const initial = useWorkbenchStore.getState();
beforeEach(async () => {
  resetBrowserDesktopForTests();
  useWorkbenchStore.setState(initial, true);
  await useWorkbenchStore.getState().initialize();
  await useWorkbenchStore.getState().openNewChatDraft();
});
afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initial, true);
});

it("starts without a project and lets the menu choose and clear the draft scope", async () => {
  useWorkbenchStore.setState({
    projects: [
      {
        id: "one",
        name: "Sage",
        parentPath: "/work/first",
        rootCount: 1,
        lastOpenedAt: 0,
      },
      {
        id: "two",
        name: "Sage",
        parentPath: "/work/second",
        rootCount: 1,
        lastOpenedAt: 0,
      },
    ],
  });
  render(<ProjectScopeChip projectName="Choose project" />);
  const chip = screen.getByRole("button", { name: "Choose project" });
  expect(chip).toHaveAttribute("title", "Change the project for this chat");
  fireEvent.click(chip);
  fireEvent.click(
    await screen.findByRole("option", { name: "Sage, /work/first" }),
  );
  expect(useWorkbenchStore.getState().newChatDraft?.targetProjectId).toBe(
    "one",
  );
  fireEvent.click(screen.getByRole("button", { name: "Project: Sage" }));
  expect(
    await screen.findByRole("option", { name: "New project" }),
  ).toBeVisible();
  fireEvent.click(
    screen.getByRole("option", { name: "Don't work in a project" }),
  );
  expect(screen.getByRole("button", { name: "Choose project" })).toBeVisible();
  expect(useWorkbenchStore.getState().newChatDraft?.targetProjectId).toBeNull();
});

it("offers a keyboard-accessible clear button and freezes the chip when disabled", async () => {
  await act(async () =>
    useWorkbenchStore
      .getState()
      .openNewChatDraft(useWorkbenchStore.getState().project!.id),
  );
  const { rerender } = render(<ProjectScopeChip projectName="Sage" />);
  fireEvent.click(
    screen.getByRole("button", { name: "Remove project from this chat" }),
  );
  expect(useWorkbenchStore.getState().newChatDraft?.targetProjectId).toBeNull();
  rerender(<ProjectScopeChip projectName="Choose project" disabled />);
  expect(screen.getByRole("button", { name: "Choose project" })).toBeDisabled();
  expect(
    screen.queryByRole("button", { name: "Remove project from this chat" }),
  ).toBeNull();
});

it("keeps an established session's scope read-only", () => {
  useWorkbenchStore.setState({
    agentCenter: "chat",
    newChatDraft: null,
    sessionId: "started-session",
  });
  render(<ProjectScopeChip projectName="Sage" />);
  const chip = screen.getByRole("button", { name: "Project: Sage" });
  expect(chip).toBeDisabled();
  fireEvent.click(chip);
  expect(screen.queryByRole("listbox")).toBeNull();
  expect(
    screen.queryByRole("button", { name: "Remove project from this chat" }),
  ).toBeNull();
});
