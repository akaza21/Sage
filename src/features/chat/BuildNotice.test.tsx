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
  expect(screen.getByRole("status")).toHaveTextContent("Build 1.2.3");
  fireEvent.click(screen.getByRole("button", { name: "Dismiss build notice" }));
  expect(screen.queryByRole("status")).toBeNull();
  rerender(<HarnessDiscoveryNotice harness={{ ...harness }} />);
  expect(screen.queryByRole("status")).toBeNull();
  rerender(
    <HarnessDiscoveryNotice harness={{ ...harness, boundVersion: "1.2.4" }} />,
  );
  expect(screen.getByRole("status")).toHaveTextContent("Build 1.2.4");
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
  expect(screen.getByRole("status")).toHaveTextContent("Build 1.2.3");
});

it("explains the build again when a chat pane is remounted", () => {
  const firstPane = render(<HarnessDiscoveryNotice harness={harness} />);
  fireEvent.click(screen.getByRole("button", { name: "Dismiss build notice" }));
  firstPane.unmount();
  render(<HarnessDiscoveryNotice harness={harness} />);
  expect(screen.getByRole("status")).toHaveTextContent("Build 1.2.3");
});

it("says a newer release keeps every feature instead of calling it uncharacterised", () => {
  render(
    <HarnessDiscoveryNotice
      harness={{ ...harness, characterisation: "newerThanTested" }}
    />,
  );
  const notice = screen.getByRole("status");
  expect(notice).toHaveTextContent(
    "Build 1.2.3 is newer than Sage has tested. Tested features stay on.",
  );
  expect(notice).not.toHaveTextContent("Some features are unavailable");
});

it("shows no build notice for the tested release", () => {
  render(
    <HarnessDiscoveryNotice
      harness={{ ...harness, characterisation: "characterised" }}
    />,
  );
  expect(screen.queryByRole("status")).toBeNull();
});
