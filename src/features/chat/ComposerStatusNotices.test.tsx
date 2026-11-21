import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import {
  ComposerStatusNotices,
  type ComposerStatusNoticeProps,
} from "./ComposerStatusNotices";

const recoveryModule = vi.hoisted(() => {
  let request!: () => void;
  let reject!: (error: Error) => void;
  return {
    requested: new Promise<void>((resolve) => {
      request = resolve;
    }),
    load: new Promise<never>((_, fail) => {
      reject = fail;
    }),
    request: () => request(),
    reject: () => reject(new Error("Recovery module unavailable")),
  };
});

vi.mock("./ComposerAddActions", () => {
  recoveryModule.request();
  return recoveryModule.load;
});

const guidance = "No reviewed local binding is available for this harness.";
const props: ComposerStatusNoticeProps = {
  selectedHarnessId: null,
  workbenchError: null,
  chatError: null,
  imageStatus: null,
  missingBindingNotice: true,
  discoveryPhase: "ready",
};

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

it("shows a shared failure once while retaining a different image failure", () => {
  render(
    <ComposerStatusNotices
      {...props}
      missingBindingNotice={false}
      workbenchError="Harness launch failed."
      chatError="Harness launch failed."
      imageStatus="Image upload failed."
    />,
  );
  expect(
    screen.getAllByRole("alert").map((alert) => alert.textContent),
  ).toEqual(["Harness launch failed.", "Image upload failed."]);
});

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

it("preserves binding guidance and unrelated errors while recovery loads and fails", async () => {
  vi.spyOn(console, "error").mockImplementation(() => undefined);
  render(
    <ComposerStatusNotices
      {...props}
      authRecovery={{
        harnessId: "fixture",
        scope: "chat-a",
        onSignedIn: () => undefined,
      }}
      signInHarness={{ name: "Harness", signIn: null }}
      workbenchError="Workspace could not be refreshed."
      chatError="The harness requires sign-in."
      imageStatus="Image upload failed."
    />,
  );
  await act(async () => {
    await recoveryModule.requested;
  });
  const expectNoticesVisible = () => {
    expect(screen.getByText(guidance)).toBeVisible();
    const alerts = screen.getAllByRole("alert");
    expect(alerts.map((alert) => alert.textContent)).toEqual([
      "Workspace could not be refreshed.",
      "The harness requires sign-in.",
      "Image upload failed.",
    ]);
    for (const alert of alerts) expect(alert).toBeVisible();
  };
  expectNoticesVisible();
  await act(async () => {
    recoveryModule.reject();
    await vi.dynamicImportSettled();
  });
  expectNoticesVisible();
});
