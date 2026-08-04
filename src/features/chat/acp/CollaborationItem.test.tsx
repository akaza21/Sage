import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { AcpItem } from "../../../domain/acp";
import { CollaborationItem } from "./CollaborationItem";

afterEach(cleanup);

describe("CollaborationItem", () => {
  it("projects harness-owned collaboration without creating Sage controls", () => {
    const item = {
      kind: "collaboration",
      payload: {
        activityId: "activity-1",
        label: "Research lane",
        summary: "Harness activity completed.",
        receiverLabels: ["Reviewer", "Builder"],
      },
    } as unknown as Extract<AcpItem, { kind: "collaboration" }>;
    render(<CollaborationItem item={item} />);
    expect(screen.getByLabelText("Harness collaboration")).toHaveTextContent(
      "Participants: Reviewer, Builder",
    );
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });
});
