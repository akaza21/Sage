import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../app/store";
import {
  ProjectChatActionMenu,
  ProjectChatTrailingState,
} from "./ProjectChatActionMenu";

const initialState = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
  vi.restoreAllMocks();
});

function RetainedActions({
  chatId = "chat:a",
  projectId = "project-a",
}: {
  chatId?: string;
  projectId?: string;
}) {
  const [open, setOpen] = useState(false);
  return (
    <ProjectChatActionMenu
      chatId={chatId}
      projectId={projectId}
      chatTitle="Task"
      pinned={false}
      open={open}
      disabled={false}
      archiveDisabled={false}
      onOpenChange={setOpen}
      onRename={vi.fn()}
      sessionChrome={{
        chatId,
        harnessStatus: "Ready",
        canCloseSession: false,
        canEndSession: false,
        busy: false,
        acpPage: null,
        messages: [],
        actions: {
          current: {
            closeSession: vi.fn(),
            endSession: vi.fn(),
            focusComposer: vi.fn(),
          },
        },
      }}
    />
  );
}

it.each(["close", "chat", "project"])(
  "resets a retained move picker on %s without remounting session actions",
  async (change) => {
    const mounted = render(<RetainedActions />);
    const trigger = screen.getByRole("button", {
      name: "Chat actions for Task",
    });
    fireEvent.click(trigger);
    fireEvent.click(
      await screen.findByRole("menuitem", { name: "Move to project" }),
    );
    expect(screen.getByText("No other project available.")).toBeVisible();
    if (change === "close") {
      fireEvent.click(trigger);
      expect(screen.queryByRole("menu")).toBeNull();
      act(() =>
        useWorkbenchStore.setState({
          projects: [
            { id: "destination", name: "Work", rootCount: 1, lastOpenedAt: 1 },
          ],
        }),
      );
      fireEvent.click(trigger);
    } else {
      mounted.rerender(
        <RetainedActions
          chatId={change === "chat" ? "chat:b" : "chat:a"}
          projectId={change === "project" ? "project-b" : "project-a"}
        />,
      );
    }
    expect(
      await screen.findByRole("menuitem", { name: "Pin chat" }),
    ).toBeVisible();
    expect(screen.queryByRole("menuitem", { name: "Back" })).toBeNull();
  },
);

it("ends a chat row with its age, a spinner while running or a dot when it needs you", () => {
  vi.spyOn(Date, "now").mockReturnValue(10 * 60_000);
  const view = render(
    <ProjectChatTrailingState
      awaitingApproval={false}
      focused={false}
      state={null}
      updatedAt={7 * 60_000}
    />,
  );
  expect(view.container).toHaveTextContent(/^3m$/);
  view.rerender(
    <ProjectChatTrailingState
      awaitingApproval={false}
      focused={false}
      state={{ kind: "working", label: "Working" }}
      updatedAt={7 * 60_000}
    />,
  );
  expect(screen.getByRole("img", { name: "Working" })).toBeInTheDocument();
  expect(view.container).not.toHaveTextContent("3m");
  view.rerender(
    <ProjectChatTrailingState
      awaitingApproval={false}
      focused={false}
      state={{ kind: "unread", label: "Unread" }}
      updatedAt={7 * 60_000}
    />,
  );
  expect(
    screen.getByRole("img", { name: "Unread updates" }),
  ).toBeInTheDocument();
});
