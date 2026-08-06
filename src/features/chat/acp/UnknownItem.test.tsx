import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { AcpItem } from "../../../domain/acp";
import { UnknownItem } from "./UnknownItem";

afterEach(cleanup);

describe("UnknownItem", () => {
  it("renders only the normalized bounded fallback", () => {
    const item = {
      kind: "unsupported",
      payload: {
        category: "extension.future",
        summary: "A future item was retained safely.",
        truncated: true,
      },
    } as Extract<AcpItem, { kind: "unsupported" }>;
    render(<UnknownItem item={item} />);
    expect(
      screen.getByRole("group", {
        name: "Unsupported tool activity",
        description: "This harness activity is not supported by Sage",
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("status", { name: "Unsupported" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Activity details")).toBeInTheDocument();
    expect(
      screen.getByText("A future item was retained safely."),
    ).toBeInTheDocument();
    expect(screen.getByText("Summary truncated")).toBeInTheDocument();
  });
});
