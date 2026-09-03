import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { AcpItem } from "../../../domain/acp";
import { CollaborationItem } from "./CollaborationItem";

afterEach(cleanup);

describe("CollaborationItem", () => {
  it("projects harness-owned collaboration without creating Sage controls", () => {
    const label =
      "Coordinate the independent accessibility review across every visible workbench surface";
    const item = {
      kind: "collaboration",
      lifecycle: "streaming",
      payload: {
        activityId: "activity-1",
        label,
        summary: "Harness activity completed.",
        receiverLabels: ["Reviewer", "Builder"],
      },
    } as unknown as Extract<AcpItem, { kind: "collaboration" }>;
    render(<CollaborationItem item={item} />);
    const collaboration = screen.getByRole("group", {
      name: `Harness collaboration: ${label}`,
      description: "Read-only activity owned and scheduled by the harness",
    });
    expect(collaboration).toHaveTextContent("Participants: Reviewer, Builder");
    expect(
      within(collaboration).getByText("Collaboration"),
    ).toBeInTheDocument();
    expect(within(collaboration).getByText(label)).toHaveAttribute(
      "title",
      label,
    );
    expect(screen.getByRole("status", { name: "Active" })).toHaveAttribute(
      "data-lifecycle",
      "streaming",
    );
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });
});
