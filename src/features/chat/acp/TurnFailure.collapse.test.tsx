import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";

import { AcpChatPresentation } from "./AcpChatPresentation";
import { page } from "./acpProjection.fixture";

afterEach(cleanup);

it("collapses a failure a later turn completed after, and dismissing it keeps it hidden", () => {
  const failed = page("failed", {
    kind: "authRequired",
    message: "Failed to authenticate: OAuth session expired",
    remediation: "Retry first.",
  });
  const completed = page("completed").turns[0];
  failed.turns = [
    { ...failed.turns[0], terminalAt: 1 },
    { ...completed, id: "turn-2" as never, ordinal: 2 },
  ];
  const view = () => (
    <AcpChatPresentation
      page={failed}
      hasEarlier={false}
      loadingEarlier={false}
      historyError={null}
      projectRoots={[]}
      retentionIncomplete={false}
    />
  );
  const { unmount } = render(view());
  const failure = screen.getByRole("group", { name: "Turn failed" });
  expect(failure).toHaveAttribute("data-latest", "false");
  expect(failure).toHaveTextContent(
    "Failed to authenticate: OAuth session expired",
  );
  fireEvent.click(screen.getByRole("button", { name: "Details" }));
  expect(failure.querySelector(".notice__details")).not.toBeNull();

  fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
  expect(screen.queryByRole("group", { name: "Turn failed" })).toBeNull();
  unmount();
  render(view());
  expect(screen.queryByRole("group", { name: "Turn failed" })).toBeNull();
});
