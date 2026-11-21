import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, it, vi } from "vitest";
import { HarnessSignInPart } from "../lazyParts";
import { page } from "./acpProjection.fixture";
import { TurnFailure } from "./TurnFailure";
import { TurnHeader } from "./TurnHeader";

vi.mock("../../harness/HarnessSignInStatus", () => {
  throw new Error("Harness view could not load");
});

afterEach(cleanup);

it.each([true, false])(
  "keeps the explanation and remediation when sign-in controls cannot load (retry=%s)",
  async (retry) => {
    await expect(HarnessSignInPart.preload()).rejects.toMatchObject({
      cause: { message: "Harness view could not load" },
    });
    function FailedChat() {
      const [retried, setRetried] = useState(false);
      const failed = page("failed", {
        kind: "authRequired",
        message: "Your harness session has expired",
        remediation: "Sign in again, then retry the turn",
      });
      const turn = failed.turns[0];
      return retried ? (
        <p>Retry requested</p>
      ) : (
        <>
          <TurnHeader turn={turn} />
          <TurnFailure
            turn={turn}
            page={retry ? failed : undefined}
            onRetry={retry ? () => setRetried(true) : undefined}
          />
        </>
      );
    }
    render(<FailedChat />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Details" }));
      await expect(HarnessSignInPart.preload()).rejects.toMatchObject({
        cause: { message: "Harness view could not load" },
      });
    });
    expect(screen.getByRole("status", { name: "Failed" })).toBeVisible();
    expect(screen.getByText("Your harness session has expired")).toBeVisible();
    expect(
      screen.getByText("Sign in again, then retry the turn"),
    ).toBeVisible();
    expect(
      await screen.findByRole("alert", {
        name: "Failure details failed to load",
      }),
    ).toHaveTextContent("Failure details could not be loaded.");
    const turnRetry = screen
      .getByRole("group", { name: "Turn failed" })
      .querySelector<HTMLButtonElement>(".notice__action");
    if (retry) {
      expect(turnRetry).toHaveAccessibleName("Retry");
      fireEvent.click(turnRetry!);
      expect(screen.getByText("Retry requested")).toBeVisible();
    } else {
      expect(turnRetry).toBeNull();
    }
  },
);
