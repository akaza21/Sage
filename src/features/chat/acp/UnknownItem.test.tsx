import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { AcpItem } from "../../../domain/acp";
import { UnknownItem } from "./UnknownItem";

afterEach(cleanup);

describe("UnknownItem", () => {
  it("shows a retained unsupported cancellation summary in primary copy", () => {
    const item = {
      kind: "unsupported",
      lifecycle: "unsupported",
      payload: {
        category: "pi_dialog_cancelled",
        summary:
          "Pi extension dialog was cancelled: this dialog is not supported",
        truncated: false,
      },
    } as Extract<AcpItem, { kind: "unsupported" }>;
    const { container } = render(<UnknownItem item={item} />);
    expect(screen.getByText(item.payload.summary)).toBeVisible();
    expect(container.querySelector("header")).toHaveTextContent(
      item.payload.summary,
    );
    expect(container.querySelector("header")).not.toHaveTextContent(
      item.payload.category,
    );
    expect(screen.getByText(item.payload.category)).not.toBeVisible();
  });

  it.each(["pi_dialog_cancelled", "extension_cancelled"])(
    "shows a cancelled activity's human-readable summary instead of %s",
    (category) => {
      const item = {
        kind: "unsupported",
        lifecycle: "cancelled",
        payload: {
          category,
          summary:
            "Pi extension dialog was cancelled: this dialog is not supported",
          truncated: false,
        },
      } as Extract<AcpItem, { kind: "unsupported" }>;
      const { container } = render(<UnknownItem item={item} />);
      expect(screen.getByRole("status").textContent).toBe(item.payload.summary);
      expect(screen.queryByText(category)).toBeNull();
      expect(screen.queryByText("Unsupported activity")).toBeNull();
      expect(container.querySelector("details")).toBeNull();
      expect(
        container.querySelector('[data-activity-state="cancelled"]'),
      ).toHaveClass("acp-lifecycle-notice");
    },
  );

  it("renders only the normalized bounded fallback", () => {
    const item = {
      kind: "unsupported",
      payload: {
        category: "extension.future",
        summary: "A future item was retained safely.",
        truncated: true,
      },
    } as Extract<AcpItem, { kind: "unsupported" }>;
    const { container } = render(<UnknownItem item={item} />);
    expect(
      screen.getByRole("group", {
        name: "Unsupported tool activity",
        description: "This harness activity is not supported by Sage",
      }),
    ).toBeInTheDocument();
    expect(screen.getByRole("status", { name: "Unsupported" })).toHaveClass(
      "acp-state",
    );
    expect(screen.getByText("Activity details")).toBeInTheDocument();
    expect(
      screen.getByText("A future item was retained safely."),
    ).toBeInTheDocument();
    expect(screen.getByText("Summary truncated")).toBeInTheDocument();
    expect(
      container.querySelector(".acp-activity-card--unknown"),
    ).toHaveAttribute("data-activity-state", "unsupported");
  });
});
