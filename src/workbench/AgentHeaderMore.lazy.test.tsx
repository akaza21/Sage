import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { AgentHeaderMore } from "./ProjectChatActionMenu";

afterEach(cleanup);

// No preload here: an ArrowUp before the menu body has loaded still opens
// it on its last item once it arrives, and the body then stays mounted.
it("opens a menu that is still loading on the requested item", async () => {
  const onChanges = vi.fn();
  render(
    <AgentHeaderMore
      scope="chat-a"
      title="Task"
      projectName="Sage"
      disabled={false}
      onChanges={onChanges}
    >
      <button type="button" role="menuitem">
        Project actions
      </button>
    </AgentHeaderMore>,
  );
  const trigger = screen.getByRole("button", {
    name: "Chat actions for Task task",
  });
  fireEvent.keyDown(trigger, { key: "ArrowUp" });
  expect(trigger).toHaveAttribute("aria-expanded", "true");
  // A cold test transform of the menu chunk can take over a second.
  const changes = await screen.findByRole(
    "menuitem",
    { name: "Changes" },
    { timeout: 4000 },
  );
  await waitFor(() => expect(changes).toHaveFocus());
  fireEvent.keyDown(changes, { key: "Escape" });
  expect(trigger).toHaveAttribute("aria-expanded", "false");
  expect(screen.getByText("Sage")).toBeInTheDocument();
  fireEvent.click(trigger);
  fireEvent.click(screen.getByRole("menuitem", { name: "Changes" }));
  expect(onChanges).toHaveBeenCalledOnce();
});
