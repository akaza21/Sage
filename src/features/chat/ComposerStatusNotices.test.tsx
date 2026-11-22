import { composerProblem } from "./ComposerStatusNotices";
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

const props: ComposerStatusNoticeProps = {
  workbenchError: null,
  chatError: null,
  imageStatus: null,
};

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

it("shows a shared failure once while retaining a different image failure", () => {
  render(
    <ComposerStatusNotices
      {...props}
      workbenchError={composerProblem(
        "launch",
        "chat-a",
        "Harness launch failed.",
      )}
      chatError={composerProblem("launch", "chat-a", "Harness launch failed.")}
      imageStatus={composerProblem(
        "image",
        "chat-a",
        "Image upload failed.",
        "attachment",
        "info",
      )}
    />,
  );
  expect(
    screen.getAllByRole("alert").map((alert) => alert.textContent),
  ).toEqual(["Error: Harness launch failed."]);
  expect(
    screen.getByText("Image upload failed.").closest(".notice"),
  ).toHaveClass("notice--info");
  expect(screen.getByRole("status")).toHaveTextContent("Image upload failed.");
  expect(screen.getByRole("status")).toHaveAttribute("aria-live", "polite");
});

it("preserves unrelated errors while recovery loads and fails", async () => {
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
      workbenchError={composerProblem(
        "workspace",
        "project-a",
        "Workspace could not be refreshed.",
      )}
      chatError={composerProblem(
        "signInRequired",
        "chat-a",
        "The harness requires sign-in.",
      )}
      imageStatus={composerProblem(
        "image",
        "chat-a",
        "Image upload failed.",
        "attachment",
        "info",
      )}
    />,
  );
  await act(async () => {
    await recoveryModule.requested;
  });
  const expectNoticesVisible = () => {
    const alerts = screen.getAllByRole("alert");
    expect(alerts.map((alert) => alert.textContent)).toEqual([
      "Error: Workspace could not be refreshed.",
      "Error: The harness requires sign-in.",
    ]);
    for (const alert of alerts) expect(alert).toBeVisible();
    expect(
      screen.getByText("Image upload failed.").closest(".notice"),
    ).toHaveClass("notice--info");
  };
  expectNoticesVisible();
  await act(async () => {
    recoveryModule.reject();
    await vi.dynamicImportSettled();
  });
  expectNoticesVisible();
});
