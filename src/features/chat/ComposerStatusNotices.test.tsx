import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import {
  ComposerStatusNotices,
  type ComposerStatusNoticeProps,
} from "./ComposerStatusNotices";

const guidance = "No reviewed local binding is available for this harness.";
const props: ComposerStatusNoticeProps = {
  selectedHarnessId: null,
  workbenchError: null,
  chatError: null,
  imageStatus: null,
  missingBindingNotice: true,
  discoveryPhase: "ready",
};

afterEach(cleanup);

it.each([
  ["idle", true],
  ["ready", true],
  ["checking", false],
  ["signInRequired", false],
  ["consent", false],
  ["waiting", false],
  ["error", false],
] as const)(
  "explains a missing binding in discovery phase %s: %s",
  (phase, visible) => {
    render(<ComposerStatusNotices {...props} discoveryPhase={phase} />);
    expect(Boolean(screen.queryByText(guidance))).toBe(visible);
  },
);

it("preserves binding guidance and unrelated errors while sign-in UI loads", () => {
  render(
    <ComposerStatusNotices
      {...props}
      signInHarness={{ name: "Harness", signIn: null }}
      workbenchError="Workspace could not be refreshed."
      chatError="The harness requires sign-in."
      imageStatus="Image upload failed."
    />,
  );
  expect(screen.getByText(guidance)).toBeVisible();
  expect(
    screen.getAllByRole("alert").map((alert) => alert.textContent),
  ).toEqual([
    "Workspace could not be refreshed.",
    "The harness requires sign-in.",
    "Image upload failed.",
  ]);
});
