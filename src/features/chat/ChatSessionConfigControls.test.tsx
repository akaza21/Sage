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
  it("keeps select groups named without exposing visual headings as menu content", () => {
    render(
      <ChatSessionConfigControls
        sessionConfig={overlayWith({
          id: "speed",
          name: "Speed",
          type: "select",
          currentValue: "fast",
          options: [{ value: "fast", name: "Fast" }],
        })}
        pending={false}
        onSetSessionConfigOption={vi.fn()}
      />,
    );
    expect(screen.getByRole("group", { name: "Session speed" })).toBeVisible();
    expect(screen.getByText("Speed", { selector: "p" })).toHaveAttribute(
      "aria-hidden",
      "true",
    );
  });

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
        onSetSessionConfigOption={vi.fn()}
      />,
    );
    expect(screen.getByText("Thinking")).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    expect(screen.queryByRole("checkbox")).toBeNull();
  });

  it("decodes a negotiated boolean into an enabled menu checkbox without a second picker", () => {
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
        onSetSessionConfigOption={onSetSessionConfigOption}
      />,
    );
    const checkbox = screen.getByRole("menuitemcheckbox", {
      name: "Session flag",
    });
    expect(checkbox).toBeChecked();
    expect(checkbox).toBeEnabled();
    expect(checkbox).toHaveTextContent("Flag");
    expect(screen.queryByRole("combobox")).toBeNull();
    fireEvent.click(checkbox);
    expect(onSetSessionConfigOption).toHaveBeenCalledWith("flag", {
      type: "boolean",
      value: false,
    });
  });
});
