import { cleanup, render } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";

import type { AgentRailChat } from "../domain/workbench";
import { resetChatReadMarks } from "../features/chat/acp/chatReadMarks";
import { CollapsedProjectStatus } from "./CollapsedProjectStatus";

afterEach(() => {
  cleanup();
  resetChatReadMarks();
});

const chat = (id: string, status: AgentRailChat["status"]) =>
  ({
    id,
    kind: "chat",
    title: id,
    position: 0,
    pinned: false,
    dirty: false,
    status,
  }) as AgentRailChat;

it("shows a collapsed project as needing attention when one chat waits, like its expanded row", () => {
  const { container } = render(
    <CollapsedProjectStatus
      chats={[chat("chat:run", "working"), chat("chat:wait", "waiting")]}
      selected={false}
    />,
  );
  expect(container.querySelector(".project-chat__state")).toHaveAccessibleName(
    "Needs attention",
  );
});
