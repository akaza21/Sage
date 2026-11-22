import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { AcpHistoryNotices } from "./AcpHistoryNotices";

afterEach(cleanup);

it("combines unloaded and retained-history limits into one boundary with one suffix", () => {
  const { container } = render(
    <AcpHistoryNotices
      hasEarlier
      loadingEarlier={false}
      historyError={null}
      unloaded
      retentionIncomplete
    />,
  );
  expect(container.querySelectorAll("[data-notice-identity]")).toHaveLength(1);
  expect(
    container.textContent?.match(
      /Changed-file summaries show retained records only\./g,
    ),
  ).toHaveLength(1);
  expect(
    screen.getByText(/Earlier history has not been loaded/),
  ).toHaveTextContent("some history was discarded");
  fireEvent.click(screen.getByRole("button", { name: "Details" }));
  expect(screen.getByText(/memory limits/)).toBeVisible();
});
