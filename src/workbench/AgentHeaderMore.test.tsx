import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { AgentHeaderMore } from "./ProjectChatActionMenu";
import { MenuPopover } from "../ui/MenuPopover";
import { useRef, useState } from "react";

afterEach(cleanup);
function NestedActions() {
  const trigger = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  return (
    <>
      <button
        ref={trigger}
        type="button"
        role="menuitem"
        onClick={() => setOpen(true)}
      >
        Project actions
      </button>
      <MenuPopover
        id="nested"
        ariaLabel="Nested actions"
        open={open}
        keepMounted
        triggerRef={trigger}
        portalTarget="closest-popover"
        onDismiss={() => setOpen(false)}
      >
        <button type="button" role="menuitem">
          Hidden action
        </button>
      </MenuPopover>
    </>
  );
}
it("skips retained nested-menu items while navigating the outer menu", () => {
  render(
    <AgentHeaderMore
      scope="chat-a"
      title="Task"
      projectName="Sage"
      disabled={false}
      onChanges={vi.fn()}
    >
      <NestedActions />
    </AgentHeaderMore>,
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Chat actions for Task task" }),
  );
  const project = screen.getByRole("menuitem", { name: "Project actions" });
  fireEvent.click(project);
  fireEvent.keyDown(screen.getByRole("menu", { name: "Nested actions" }), {
    key: "Escape",
  });
  fireEvent.keyDown(project, { key: "ArrowDown" });
  expect(screen.getByRole("menuitem", { name: "Changes" })).toHaveFocus();
});
it("opens the last item with ArrowUp", () => {
  render(
    <AgentHeaderMore
      scope="chat-a"
      title="Task"
      projectName="Sage"
      disabled={false}
      onChanges={vi.fn()}
    >
      <button type="button" role="menuitem">
        Project actions
      </button>
    </AgentHeaderMore>,
  );
  fireEvent.keyDown(
    screen.getByRole("button", { name: "Chat actions for Task task" }),
    { key: "ArrowUp" },
  );
  expect(screen.getByRole("menuitem", { name: "Changes" })).toHaveFocus();
});
it("shows the real project and Changes, and restores Escape focus", () => {
  const onChanges = vi.fn();
  render(
    <AgentHeaderMore
      scope="chat-a"
      title="Task"
      projectName="Sage"
      disabled={false}
      onChanges={onChanges}
    >
      <button type="button">Project actions</button>
    </AgentHeaderMore>,
  );
  const trigger = screen.getByRole("button", {
    name: "Chat actions for Task task",
  });
  fireEvent.click(trigger);
  expect(screen.getByText("Sage")).toBeVisible();
  expect(screen.getByRole("button", { name: "Project actions" })).toBeVisible();
  fireEvent.click(screen.getByRole("menuitem", { name: "Changes" }));
  expect(onChanges).toHaveBeenCalledOnce();
  expect(trigger).toHaveFocus();
  fireEvent.click(trigger);
  fireEvent.keyDown(screen.getByRole("menu", { name: "Chat tools" }), {
    key: "Escape",
  });
  expect(trigger).toHaveAttribute("aria-expanded", "false");
  expect(trigger).toHaveFocus();
});
