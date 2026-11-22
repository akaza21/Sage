import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import frame from "../../../src-tauri/testdata/pi/extension-commands.json";
import { ComposerExtensions } from "./ComposerExtensions";
import { decodeCommandList } from "../../services/acpCommandsDesktop";
import { safeItem } from "../../services/acpDesktopResponseItems";
import { ExtensionMessage } from "./acp/ExtensionMessage";

afterEach(cleanup);

it("shows get_commands provenance without attributing unrelated resources and searches within 100 rows", () => {
  // Pi 1.5.0 get_commands response, with the fixture root substituted for privacy.
  const list = decodeCommandList({
    revision: 1,
    commands: frame.data.commands.map((command) => ({
      ...command,
      inputHint: null,
    })),
  });
  expect(list).not.toBeNull();
  const catalog = [
    ...list!.commands,
    ...Array.from({ length: 110 }, (_, index) => ({
      name: `command-${index}`,
      description: "No provider attribution",
      source: "extension" as const,
      inputHint: null,
    })),
  ];
  render(<ComposerExtensions catalog={catalog} page={null} />);
  expect(
    within(
      screen.getByRole("region", { name: "Extensions and packages" }),
    ).getByText(
      /\/fixture\/profile\/extensions\/render.js · user · top-level · auto/,
    ),
  ).toBeVisible();
  expect(screen.getAllByRole("listitem")).toHaveLength(100);
  expect(
    screen.getByText("Showing 100 rows. Refine your search to see more."),
  ).toBeVisible();
  fireEvent.change(screen.getByRole("searchbox"), {
    target: { value: "command-109" },
  });
  expect(screen.getAllByRole("listitem")).toHaveLength(1);
  expect(
    within(screen.getByRole("region", { name: "From harness" })).getByText(
      "/command-109",
    ),
  ).toBeVisible();
});

it("renders a projected custom message as safe text with disposable details", () => {
  const item = safeItem({
    id: "extension-message",
    projectId: "project-1",
    chatId: "chat-1",
    sessionId: "session-1",
    turnId: "turn-1",
    runtimeEpoch: "1",
    revision: "1",
    ordinal: 1,
    origin: "primary",
    activityId: null,
    lifecycle: "completed",
    createdAt: 1,
    updatedAt: 1,
    terminalAt: 1,
    kind: "extensionMessage",
    toolState: null,
    payload: {
      label: "fixture-report",
      text: "<script>plain text</script>",
      details: "<img src=x onerror=alert(1)>",
    },
  });
  if (item?.kind !== "extensionMessage") throw new Error("Invalid projection");
  const { container } = render(<ExtensionMessage item={item} />);
  expect(screen.getByText("<script>plain text</script>")).toBeVisible();
  expect(container.querySelector("script, img")).toBeNull();
  expect(screen.queryByText(item.payload.details!)).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Details" }));
  expect(screen.getByText(item.payload.details!)).toBeVisible();
  expect(container.querySelector("script, img")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Details" }));
  expect(container.querySelector("pre")).toBeNull();
});
