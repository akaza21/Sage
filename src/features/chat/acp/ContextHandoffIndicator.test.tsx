import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { HandoffSummary } from "../../../domain/acp";
import { ContextHandoffIndicator } from "./ContextHandoffIndicator";

afterEach(cleanup);

const summary: HandoffSummary = {
  id: "handoff",
  revision: "4",
  targetSession: "session",
  turnId: "turn",
  state: "inline",
  carriedItems: 3,
  carriedBytes: 700,
  omissions: { unsupportedKind: 2, oversize: 1, scanLimit: 5, budget: 4 },
};

describe("context handoff coverage", () => {
  it("handoffIndicatorShowsCarriedAndOmitted", () => {
    render(<ContextHandoffIndicator summary={summary} />);
    expect(screen.getByRole("status")).toHaveTextContent("3 items carried");
    expect(screen.getByRole("status")).toHaveTextContent("12 omitted");
    expect(screen.getByRole("status")).toHaveTextContent("700 bytes");
  });

  it("unknownDeliveryNeverClaimsDelivered", () => {
    render(
      <ContextHandoffIndicator summary={{ ...summary, state: "unknown" }} />,
    );
    expect(screen.getByRole("status")).toHaveTextContent("Delivery unknown");
    expect(screen.getByRole("status")).toHaveTextContent("may have occurred");
    expect(
      screen.queryByText(/items carried|delivered/i),
    ).not.toBeInTheDocument();
  });
});
