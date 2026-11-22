import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { composerMenuSections } from "./composerMenuSections";
import { composerNativeRows } from "./composerNativeRows";
import { ComposerMenuSections } from "./ComposerSectionList";
import type { AvailableAcpCommand } from "../../services/acpCommandsDesktop";

afterEach(cleanup);
const noop = () => {};
const command = (
  name: string,
  section: NonNullable<AvailableAcpCommand["placement"]>["section"],
  source: AvailableAcpCommand["source"] = "builtin",
  sourceLabel?: string,
): AvailableAcpCommand => ({
  name,
  description: `Run ${name}`,
  inputHint: null,
  source,
  sourceLabel,
  placement: { section },
});

it.each([
  { label: "empty", capabilities: false, catalog: [], expected: [], query: "" },
  {
    label: "wired and reported",
    capabilities: true,
    catalog: [
      command("inspect", "more", "plugin", "Audit"),
      command("search", "connections", "mcp"),
      command("reload", "addOns"),
      command("recipe", "skillsAndCommands", "skill"),
      command("second", "more", "plugin", "Build"),
      command("first", "more", "builtin"),
      command("third", "more", "plugin", "Audit"),
      {
        ...command("servers", "connections"),
        placement: {
          section: "connections" as const,
          control: "mcpPanel" as const,
        },
      },
    ],
    expected: [
      "Add to message",
      "Session",
      "Connections",
      "Add-ons",
      "Skills and commands",
      "Model and access",
      "Usage",
      "More",
    ],
    query: "",
  },
  {
    label: "reported only hides empty sections",
    capabilities: false,
    catalog: [
      command("connect", "connections"),
      command("unknown", "more", "harness"),
    ],
    expected: ["Connections", "More"],
    query: "",
  },
  {
    label: "search finds the native control by its catalog name",
    capabilities: true,
    catalog: [
      {
        ...command("servers", "connections"),
        placement: {
          section: "connections" as const,
          control: "mcpPanel" as const,
        },
      },
    ],
    expected: ["Connections"],
    query: "servers",
  },
  {
    label: "search includes an overflowed row",
    capabilities: true,
    catalog: Array.from({ length: 54 }, (_, index) =>
      command(`task-${index}`, "more", "plugin", "Tools"),
    ),
    expected: ["More"],
    query: "task-53",
  },
  {
    label: "bounded sections",
    capabilities: false,
    catalog: Array.from({ length: 54 }, (_, index) =>
      command(`task-${index}`, "more", "plugin", "Tools"),
    ),
    expected: ["More"],
    query: "",
  },
])(
  "renders $label capabilities and catalog in section order",
  ({ capabilities, catalog, expected, query }) => {
    const native = composerNativeRows({
      sessionConfig: capabilities
        ? {
            sageAgentSessionId: "s",
            runtimeEpoch: "1",
            configRevision: "1",
            configOptions: [],
            modes: {
              currentModeId: "plan",
              availableModes: [{ id: "plan", name: "Plan" }],
            },
          }
        : null,
      disabled: false,
      pending: false,
      onClose: noop,
      onSetSessionMode: async () => {},
      onSetSessionConfigOption: async () => {},
      onAttachFiles: capabilities ? noop : undefined,
      imageAction: capabilities
        ? { disabled: false, onChoose: noop }
        : undefined,
      onOpenGoal: capabilities ? noop : undefined,
      onCompact: capabilities ? noop : undefined,
      onOpenUsage: capabilities ? noop : undefined,
      onOpenMcp: capabilities ? noop : undefined,
      importable: false,
      onImport: noop,
    });
    const sections = composerMenuSections(native, catalog, query);
    const { container } = render(
      <ComposerMenuSections
        sections={sections}
        disabled={false}
        onPick={noop}
      />,
    );
    expect(
      [...container.children].map((element) =>
        element.getAttribute("aria-label"),
      ),
    ).toEqual(expected);
    const more = screen.queryByRole("group", { name: "More" });
    if (capabilities && !query) {
      expect(
        screen.getByRole("menuitem", { name: "Add images" }),
      ).toBeEnabled();
      expect(
        screen.getByRole("menuitem", { name: "MCP servers" }),
      ).toBeVisible();
      expect(screen.queryByRole("menuitem", { name: "/servers" })).toBeNull();
    }
    if (catalog.some((entry) => entry.name === "inspect")) {
      expect(
        within(more!)
          .getAllByRole("group")
          .map((group) => group.getAttribute("aria-label")),
      ).toEqual(["Commands", "Plugins · Audit", "Plugins · Build"]);
      expect(
        within(more!)
          .getAllByRole("menuitem")
          .map((row) => row.textContent),
      ).toEqual([
        "/firstRun first",
        "/inspectRun inspect",
        "/thirdRun third",
        "/secondRun second",
      ]);
    }
    if (catalog.length === 54) {
      expect(within(more!).getAllByRole("menuitem")).toHaveLength(
        query ? 1 : 50,
      );
      if (query)
        expect(
          within(more!).getByRole("menuitem", { name: "/task-53" }),
        ).toBeVisible();
      else
        expect(
          within(more!).getByText("and 4 more, type to search"),
        ).toBeVisible();
    }
  },
);
