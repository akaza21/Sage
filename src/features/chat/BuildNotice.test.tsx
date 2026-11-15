import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import type { HarnessDefinition } from "../../domain/harness";
import { HarnessDiscoveryNotice } from "./HarnessDiscoveryNotice";

afterEach(cleanup);

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
  expect(screen.getByRole("status")).toHaveTextContent("Fixture agent 1.2.3");
  fireEvent.click(screen.getByRole("button", { name: "Dismiss build notice" }));
  expect(screen.queryByRole("status")).toBeNull();
  rerender(<HarnessDiscoveryNotice harness={{ ...harness }} />);
  expect(screen.queryByRole("status")).toBeNull();
  rerender(
    <HarnessDiscoveryNotice harness={{ ...harness, boundVersion: "1.2.4" }} />,
  );
  expect(screen.getByRole("status")).toHaveTextContent("Fixture agent 1.2.4");
});

it("keeps an unavailable harness explanation visible after dismissing a build", () => {
  const { rerender } = render(<HarnessDiscoveryNotice harness={harness} />);
  fireEvent.click(screen.getByRole("button", { name: "Dismiss build notice" }));
  rerender(
    <HarnessDiscoveryNotice
      harness={{ ...harness, installState: "unavailable" }}
    />,
  );
  expect(screen.getByRole("status")).toHaveTextContent(
    "Sage can't start this harness",
  );
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
  expect(screen.getByRole("status")).toHaveTextContent("Fixture agent 1.2.3");
});

it("explains the build again when a chat pane is remounted", () => {
  const firstPane = render(<HarnessDiscoveryNotice harness={harness} />);
  fireEvent.click(screen.getByRole("button", { name: "Dismiss build notice" }));
  firstPane.unmount();
  render(<HarnessDiscoveryNotice harness={harness} />);
  expect(screen.getByRole("status")).toHaveTextContent("Fixture agent 1.2.3");
});

it("says a newer release keeps the tested features instead of calling it untested", () => {
  render(
    <HarnessDiscoveryNotice
      harness={{ ...harness, characterisation: "newerThanTested" }}
    />,
  );
  const notice = screen.getByRole("status");
  expect(notice).toHaveTextContent(
    "Fixture agent 1.2.3 is newer than Sage has tested. Tested features stay on.",
  );
  expect(notice).not.toHaveTextContent("some features are off");
});

it("shows no build notice for the tested release", () => {
  render(
    <HarnessDiscoveryNotice
      harness={{ ...harness, characterisation: "characterised" }}
    />,
  );
  expect(screen.queryByRole("status")).toBeNull();
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
  expect(screen.getByRole("status")).toHaveTextContent(
    "This version is too old for Sage. Update it, then refresh harness discovery in Settings.",
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
  const notice = screen.getByRole("status");
  expect(notice).toHaveTextContent(
    "Fixture agent 1.3.0 is available. Run npm install -g fixture-agent@latest, then refresh discovery in Settings.",
  );
  expect(notice).not.toHaveTextContent("tested");
  fireEvent.click(screen.getByRole("button", { name: "Dismiss build notice" }));
  expect(screen.queryByRole("status")).toBeNull();
  rerender(
    <HarnessDiscoveryNotice
      harness={{
        ...harness,
        characterisation: "characterised",
        update: { ...update, latestVersion: "1.4.0" },
      }}
    />,
  );
  expect(screen.getByRole("status")).toHaveTextContent(
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
  expect(screen.getByRole("status")).toHaveTextContent(
    "This version is too old for Sage. Run npm install -g fixture-agent@latest, then refresh discovery in Settings.",
  );
});

it("keeps a dismissed build explanation away when an update is found later", () => {
  const { rerender } = render(<HarnessDiscoveryNotice harness={harness} />);
  fireEvent.click(screen.getByRole("button", { name: "Dismiss build notice" }));
  rerender(<HarnessDiscoveryNotice harness={{ ...harness, update }} />);
  const notice = screen.getByRole("status");
  expect(notice).toHaveTextContent("Fixture agent 1.3.0 is available.");
  expect(notice).not.toHaveTextContent("hasn't been tested");
});
