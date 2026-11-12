import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { AcpItem, HarnessActivityProjection } from "../../../domain/acp";
import { HarnessActivity } from "./HarnessActivity";

afterEach(cleanup);

describe("HarnessActivity", () => {
  it("groups bounded child items in a roving keyboard tree", () => {
    const activityLabel =
      "Coordinate the independent accessibility review across every visible workbench surface";
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
        safeLabel: activityLabel,
      },
      revision: "1",
    } as HarnessActivityProjection;
    const firstItem = {
      id: "item-child",
      kind: "collaboration",
      activityId: "activity-1",
      lifecycle: "streaming",
      payload: {
        activityId: "activity-1",
        label: "Child result",
        summary: "Done",
        receiverLabels: [],
      },
    } as unknown as Extract<AcpItem, { kind: "collaboration" }>;
    const secondItem = {
      ...firstItem,
      id: "item-child-2",
      payload: { ...firstItem.payload, label: "Second result" },
    } as typeof firstItem;
    const { rerender } = render(
      <HarnessActivity activity={activity} items={[firstItem, secondItem]} />,
    );
    const activityLabelElement = screen.getByText(activityLabel);
    const disclosure = activityLabelElement.closest("summary");
    expect(disclosure).not.toBeNull();
    expect(activityLabelElement).toHaveAttribute("title", activityLabel);
    expect(within(disclosure!).getByText("Child activity")).toBeInTheDocument();
    const status = screen.getByRole("status", { name: "Completed" });
    expect(status).toHaveTextContent("Completed");
    expect(status).toHaveAttribute("data-lifecycle", "completed");
    fireEvent.click(disclosure!);
    expect(
      screen.getByRole("group", {
        name: `Harness activity: ${activityLabel}`,
        description: "Bounded read-only child activity in the originating turn",
      }),
    ).toBeInTheDocument();
    const tree = screen.getByRole("tree");
    const nodes = screen.getAllByRole("treeitem");
    expect(nodes).toHaveLength(2);
    expect(nodes[0]).toHaveAttribute("aria-level", "2");
    expect(nodes[0]).toHaveAttribute("tabindex", "0");
    expect(nodes[1]).toHaveAttribute("tabindex", "-1");
    expect(tree.querySelectorAll('[tabindex="0"]')).toHaveLength(1);
    nodes[0].focus();
    fireEvent.keyDown(nodes[0], { key: "ArrowDown" });
    expect(nodes[1]).toHaveFocus();
    fireEvent.keyDown(nodes[1], { key: "Home" });
    expect(nodes[0]).toHaveFocus();
    fireEvent.keyDown(nodes[0], { key: "End" });
    expect(nodes[1]).toHaveFocus();
    fireEvent.keyDown(nodes[1], { key: "ArrowLeft" });
    expect(disclosure).toHaveFocus();
    expect(disclosure?.parentElement).toHaveAttribute("open");

    nodes[1].focus();
    rerender(<HarnessActivity activity={activity} items={[firstItem]} />);
    expect(disclosure).toHaveFocus();
    expect(screen.getAllByRole("treeitem")).toHaveLength(1);
    expect(screen.getByRole("treeitem")).toHaveAttribute("tabindex", "0");

    screen.getByRole("treeitem").focus();
    rerender(<HarnessActivity activity={activity} items={[]} />);
    expect(screen.queryByRole("treeitem")).toBeNull();
    expect(disclosure).toHaveFocus();

    rerender(<HarnessActivity activity={activity} items={[secondItem]} />);
    expect(screen.getByRole("treeitem")).toHaveAttribute("tabindex", "0");
    expect(
      screen.getByText("Harness-owned child activity · level 2"),
    ).toBeInTheDocument();
  });
});
