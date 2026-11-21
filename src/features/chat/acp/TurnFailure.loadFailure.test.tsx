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

it("keeps the failure state and retry available when its details cannot load", async () => {
  await expect(HarnessSignInPart.preload()).rejects.toMatchObject({
    cause: { message: "Harness view could not load" },
  });
  function FailedChat() {
    const [retried, setRetried] = useState(false);
    const turn = page("failed", { kind: "protocolError" }).turns[0];
    return retried ? (
      <p>Retry requested</p>
    ) : (
      <>
        <TurnHeader turn={turn} />
        <TurnFailure turn={turn} onRetry={() => setRetried(true)} />
      </>
    );
  }
  await act(async () => {
    render(<FailedChat />);
    await expect(HarnessSignInPart.preload()).rejects.toMatchObject({
      cause: { message: "Harness view could not load" },
    });
  });
  expect(screen.getByRole("status", { name: "Failed" })).toBeVisible();
  fireEvent.click(await screen.findByRole("button", { name: "Retry" }));
  expect(screen.getByText("Retry requested")).toBeVisible();
});
