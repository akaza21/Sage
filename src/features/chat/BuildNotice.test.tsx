import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import type { HarnessDefinition } from "../../domain/harness";
import { HarnessDiscoveryNotice } from "./HarnessDiscoveryNotice";

afterEach(cleanup);

it("separates compatibility warning from update information", () => {
  render(<HarnessDiscoveryNotice harness={{ ...harness, update }} />);
  const compatibility = screen
    .getByText(/hasn't been tested/)
    .closest(".notice");
  const available = screen
    .getByText("Fixture agent 1.3.0 is available.")
    .closest(".notice");
  expect(compatibility).toHaveClass("notice--warning");
  expect(compatibility).not.toHaveTextContent("1.3.0");
  expect(available).toHaveClass("notice--info");
  fireEvent.click(screen.getByRole("button", { name: "Dismiss build notice" }));
  expect(screen.queryByText(/hasn't been tested/)).toBeNull();
  expect(available).toBeVisible();
});
const currentNotice = () =>
  screen
    .getByText(
      /Fixture agent [0-9]|Fixture agent needs an update|Fixture agent can't start/,
    )
    .closest(".notice")!;
const absentNotice = () =>
  screen.queryByText(
    /Fixture agent [0-9]|Fixture agent needs an update|Fixture agent can't start/,
  );

const harness: HarnessDefinition = {
  id: "fixture-agent",
  name: "Fixture agent",
  description: "ACP",
  transport: "acp",
  source: "builtIn",
  installState: "available",
  bindingId: "fixture-binding",
  boundVersion: "1.2.3",
  characterisation: "uncharacterised",
  accountModelDiscovery: false,
};

it("dismisses the current build notice and shows a different build", () => {
  const { rerender } = render(<HarnessDiscoveryNotice harness={harness} />);
  expect(currentNotice()).toHaveTextContent("Fixture agent 1.2.3");
  fireEvent.click(screen.getByRole("button", { name: "Dismiss build notice" }));
  expect(absentNotice()).toBeNull();
  rerender(<HarnessDiscoveryNotice harness={{ ...harness }} />);
  expect(absentNotice()).toBeNull();
  rerender(
    <HarnessDiscoveryNotice harness={{ ...harness, boundVersion: "1.2.4" }} />,
  );
  expect(currentNotice()).toHaveTextContent("Fixture agent 1.2.4");
});

it("keeps an unavailable harness explanation visible after dismissing a build", () => {
  const { rerender } = render(<HarnessDiscoveryNotice harness={harness} />);
  fireEvent.click(screen.getByRole("button", { name: "Dismiss build notice" }));
  rerender(
    <HarnessDiscoveryNotice
      harness={{ ...harness, installState: "unavailable" }}
    />,
  );
  expect(currentNotice()).toHaveTextContent("Fixture agent can't start yet.");
  expect(
    screen.queryByRole("button", { name: "Dismiss build notice" }),
  ).toBeNull();
});

it("shows another harness's notice even when the build label is identical", () => {
  const { rerender } = render(<HarnessDiscoveryNotice harness={harness} />);
  fireEvent.click(screen.getByRole("button", { name: "Dismiss build notice" }));
  rerender(
    <HarnessDiscoveryNotice harness={{ ...harness, id: "another-agent" }} />,
  );
  expect(currentNotice()).toHaveTextContent("Fixture agent 1.2.3");
});

it("explains the build again when a chat pane is remounted", () => {
  const firstPane = render(<HarnessDiscoveryNotice harness={harness} />);
  fireEvent.click(screen.getByRole("button", { name: "Dismiss build notice" }));
  firstPane.unmount();
  render(<HarnessDiscoveryNotice harness={harness} />);
  expect(currentNotice()).toHaveTextContent("Fixture agent 1.2.3");
});

it("says a newer release keeps the tested features instead of calling it untested", () => {
  render(
    <HarnessDiscoveryNotice
      harness={{ ...harness, characterisation: "newerThanTested" }}
    />,
  );
  const notice = currentNotice();
  expect(notice).toHaveTextContent(
    "Fixture agent 1.2.3 is newer than tested; tested features stay on.",
  );
  expect(notice).not.toHaveTextContent("some features are off");
});

it("shows no build notice for the tested release", () => {
  render(
    <HarnessDiscoveryNotice
      harness={{ ...harness, characterisation: "characterised" }}
    />,
  );
  expect(absentNotice()).toBeNull();
});

it("tells the user to update a release older than Sage supports", () => {
  render(
    <HarnessDiscoveryNotice
      harness={{
        ...harness,
        installState: "unavailable",
        unavailableReason: "versionTooOld",
      }}
    />,
  );
  expect(currentNotice()).toHaveTextContent(
    "Fixture agent needs an update for Sage.",
  );
});

const update = {
  latestVersion: "1.3.0",
  command: "npm install -g fixture-agent@latest",
};

it("names a published update and the command that installs it", () => {
  const { rerender } = render(
    <HarnessDiscoveryNotice
      harness={{ ...harness, characterisation: "characterised", update }}
    />,
  );
  const notice = currentNotice();
  expect(notice).toHaveTextContent("Fixture agent 1.3.0 is available.");
  expect(notice).not.toHaveTextContent("tested");
  expect(screen.queryByText(update.command)).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Details" }));
  expect(screen.getByText(update.command)).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Dismiss update" }));
  expect(absentNotice()).toBeNull();
  rerender(
    <HarnessDiscoveryNotice
      harness={{
        ...harness,
        characterisation: "characterised",
        update: { ...update, latestVersion: "1.4.0" },
      }}
    />,
  );
  expect(currentNotice()).toHaveTextContent(
    "Fixture agent 1.4.0 is available.",
  );
});

it("tells a too-old install the exact update command when one is known", () => {
  render(
    <HarnessDiscoveryNotice
      harness={{
        ...harness,
        installState: "unavailable",
        unavailableReason: "versionTooOld",
        update,
      }}
    />,
  );
  expect(currentNotice()).toHaveTextContent(
    "Fixture agent needs an update for Sage.",
  );
  fireEvent.click(screen.getByRole("button", { name: "Details" }));
  expect(screen.getByText(update.command)).toBeVisible();
});

it("keeps a dismissed build explanation away when an update is found later", () => {
  const { rerender } = render(<HarnessDiscoveryNotice harness={harness} />);
  fireEvent.click(screen.getByRole("button", { name: "Dismiss build notice" }));
  rerender(<HarnessDiscoveryNotice harness={{ ...harness, update }} />);
  const notice = currentNotice();
  expect(notice).toHaveTextContent("Fixture agent 1.3.0 is available.");
  expect(notice).not.toHaveTextContent("hasn't been tested");
});
