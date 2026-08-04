import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { AcpItem, HarnessActivityProjection } from "../../../domain/acp";
import { HarnessActivity } from "./HarnessActivity";

afterEach(cleanup);

describe("HarnessActivity", () => {
  it("groups bounded child items inside a disclosure", () => {
    const activity = {
      id: "activity-1",
      turnId: "turn-1",
      parentActivityId: null,
      parentItemId: "item-parent",
      role: "child",
      lifecycle: "completed",
      depth: 2,
      provenance: {
        schemaVersion: 1,
        sourceKind: "tool",
        externalKind: "child",
        extensionSchemaId: null,
        safeLabel: "Harness child",
      },
      revision: "1",
    } as HarnessActivityProjection;
    const item = {
      id: "item-child",
      kind: "collaboration",
      activityId: "activity-1",
      payload: {
        activityId: "activity-1",
        label: "Child result",
        summary: "Done",
        receiverLabels: [],
      },
    } as unknown as Extract<AcpItem, { kind: "collaboration" }>;
    render(<HarnessActivity activity={activity} items={[item]} />);
    expect(screen.getByText("Harness child")).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Completed");
    expect(
      screen.getByText("Harness-owned child activity · depth 2"),
    ).toBeInTheDocument();
  });
});
