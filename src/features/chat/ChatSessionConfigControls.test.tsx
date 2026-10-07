import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { decodeSessionConfig } from "../../domain/acpSessionConfig";
import { ChatSessionConfigControls } from "./ChatSessionConfigControls";

function overlayWith(option: Record<string, unknown>) {
  const decoded = decodeSessionConfig({
    sageAgentSessionId: "session-1",
    runtimeEpoch: "1",
    configRevision: "1",
    modes: null,
    configOptions: [option],
  });
  if (!decoded) throw new Error("overlay failed to decode");
  return decoded;
}

describe("ChatSessionConfigControls", () => {
  it("renders a read-only chip instead of a checkbox for a wire-demoted boolean", () => {
    render(
      <ChatSessionConfigControls
        sessionConfig={overlayWith({
          id: "thinking",
          name: "Thinking",
          type: "unsupported",
          kind: "boolean",
          currentValue: true,
        })}
        pending={false}
        error={null}
        onSetSessionConfigOption={vi.fn()}
      />,
    );
    expect(screen.getByText("Thinking")).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    expect(screen.queryByRole("checkbox")).toBeNull();
  });

  it("decodes a negotiated boolean off the wire into an enabled checkbox", () => {
    const onSetSessionConfigOption = vi.fn();
    render(
      <ChatSessionConfigControls
        sessionConfig={overlayWith({
          id: "flag",
          name: "Flag",
          type: "boolean",
          currentValue: true,
        })}
        pending={false}
        error={null}
        onSetSessionConfigOption={onSetSessionConfigOption}
      />,
    );
    const checkbox = screen.getByRole("checkbox", { name: "Session flag" });
    expect(checkbox).toBeChecked();
    expect(checkbox).toBeEnabled();
    expect(screen.queryByText("Flag")).toBeNull();
    fireEvent.click(checkbox);
    expect(onSetSessionConfigOption).toHaveBeenCalledWith("flag", {
      type: "boolean",
      value: false,
    });
  });
});
