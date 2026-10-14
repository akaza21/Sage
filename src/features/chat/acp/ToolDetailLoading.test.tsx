import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import type { AcpItem } from "../../../domain/acp";
import { ToolItem } from "./ToolItem";

afterEach(cleanup);

it("renders tool resources and command output synchronously without per-row loading UI", () => {
  const item = {
    id: "command",
    projectId: "project",
    chatId: "chat",
    sessionId: "session",
    turnId: "turn",
    runtimeEpoch: "1",
    ordinal: 1,
    origin: "primary",
    activityId: null,
    lifecycle: "completed",
    revision: "1",
    createdAt: 1,
    updatedAt: 2,
    terminalAt: 2,
    kind: "toolCall",
    toolState: "completed",
    payload: {
      inputSummary: "npm run check",
      outputSummary: "Bounded output",
      presentation: {
        kind: "execute",
        label: "Run checks",
        summary: null,
        cwd: null,
        exitCode: 0,
        outputTail: null,
        outputTruncated: false,
        resources: [
          {
            kind: "resource",
            resource: { rootId: "root", relativePath: "a.ts" },
          },
        ],
        activity: "ordinary",
      },
    },
  } as unknown as Extract<AcpItem, { kind: "toolCall" }>;
  render(<ToolItem item={item} />);
  const card = screen.getByRole("group", { name: "Run checks" });
  const output = screen.getByLabelText("Command output");
  expect(output).toHaveTextContent("Bounded output");
  expect(screen.getByLabelText("Command line")).toHaveTextContent(
    "$ npm run check",
  );
  expect(card).toHaveAttribute("aria-describedby", output.id);
  expect(document.querySelectorAll(`[id="${output.id}"]`)).toHaveLength(1);
  expect(screen.getByText("a.ts")).toBeVisible();
  expect(
    screen.getByRole("list", { name: "Referenced resources" }),
  ).toBeVisible();
  expect(screen.getAllByRole("status")).toHaveLength(1);
  expect(screen.queryByText("Loading Tool details…")).not.toBeInTheDocument();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "Retry" }),
  ).not.toBeInTheDocument();
});
