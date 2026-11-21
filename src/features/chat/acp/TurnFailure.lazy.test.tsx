import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, it } from "vitest";
import { page } from "./acpProjection.fixture";
import { TurnFailure } from "./TurnFailure";

afterEach(cleanup);

it("loads a failed turn's details and preserves its retry action", async () => {
  function FailedChat() {
    const [retry, setRetry] = useState(false);
    return retry ? (
      <p>Retry requested</p>
    ) : (
      <TurnFailure
        turn={
          page("failed", {
            kind: "protocolError",
            message: "Invalid response from harness",
            remediation: "Refresh the harness, then retry",
          }).turns[0]
        }
        onRetry={() => setRetry(true)}
      />
    );
  }
  render(<FailedChat />);
  const failure = await screen.findByRole("group", { name: "Turn failed" });
  expect(failure).toHaveTextContent("Invalid response from harness");
  expect(failure).toHaveTextContent("Refresh the harness, then retry");
  fireEvent.click(within(failure).getByRole("button", { name: "Retry" }));
  expect(screen.getByText("Retry requested")).toBeVisible();
});
