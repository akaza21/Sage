import { fireEvent, render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";

import { browserOperationFixture } from "../../domain/browserOperation.fixture";
import { BrowserAgentPresence } from "./BrowserAttachmentControls";

it("names the chat using the tab and offers one-click Stop / Take over", () => {
  const onDetach = vi.fn();
  render(
    <BrowserAgentPresence
      operation={{ ...browserOperationFixture, phase: "started" }}
      chatTitle="Research chat"
      onDetach={onDetach}
    />,
  );
  expect(screen.getByRole("status")).toHaveTextContent(
    "Agent has a browser action pending · Research chat",
  );
  fireEvent.click(screen.getByRole("button", { name: "Stop / Take over" }));
  expect(onDetach).toHaveBeenCalledOnce();
});
