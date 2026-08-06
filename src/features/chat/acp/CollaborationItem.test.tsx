import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { AcpItem } from "../../../domain/acp";
import { CollaborationItem } from "./CollaborationItem";

afterEach(cleanup);

describe("CollaborationItem", () => {
  it("projects harness-owned collaboration without creating Sage controls", () => {
    const item = {
      kind: "collaboration",
      lifecycle: "streaming",
      payload: {
        activityId: "activity-1",
        label: "Research lane",
        summary: "Harness activity completed.",
        receiverLabels: ["Reviewer", "Builder"],
      },
    } as unknown as Extract<AcpItem, { kind: "collaboration" }>;
    render(<CollaborationItem item={item} />);
    expect(
      screen.getByRole("group", {
        name: "Harness collaboration",
        description: "Read-only activity owned and scheduled by the harness",
      }),
    ).toHaveTextContent("Participants: Reviewer, Builder");
    expect(screen.getByRole("status", { name: "Active" })).toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });
});
