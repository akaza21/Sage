import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invokeNative, isTauriRuntime } from "../../ports/nativeInvoke";
import CustomAgentSettings, {
  draftArguments,
  draftEnvironmentNames,
  type CustomAcpAgent,
} from "./CustomAgentSettings";

vi.mock("../../ports/nativeInvoke", () => ({
  isTauriRuntime: vi.fn(() => true),
  invokeNative: vi.fn(),
}));

const invoke = vi.mocked(invokeNative);

function agent(slot: number): CustomAcpAgent {
  return {
    id: `custom-acp-${slot}`,
    name: `Agent ${slot}`,
    executable: `C:\\agents\\agent-${slot}.exe`,
    args: ["--acp"],
    env: ["AGENT_KEY"],
  };
}

beforeEach(() => {
  vi.mocked(isTauriRuntime).mockReturnValue(true);
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

it("keeps each argument whole and reads variable names, never values", () => {
  expect(draftArguments("--acp\n  --model fast model  \n\n")).toEqual([
    "--acp",
    "--model fast model",
  ]);
  expect(draftEnvironmentNames("AGENT_KEY, OTHER_KEY\nTHIRD")).toEqual([
    "AGENT_KEY",
    "OTHER_KEY",
    "THIRD",
  ]);
});

it("adds an agent with its arguments and passed-through names", async () => {
  const saved = agent(1);
  invoke.mockImplementation(async (command) =>
    command === "custom_acp_agents" ? [] : [saved],
  );
  render(<CustomAgentSettings />);
  fireEvent.click(await screen.findByRole("button", { name: "Add ACP agent" }));
  fireEvent.change(screen.getByLabelText("Name"), {
    target: { value: " Agent 1 " },
  });
  fireEvent.change(screen.getByLabelText("Executable path"), {
    target: { value: saved.executable },
  });
  const argumentsField = screen.getByLabelText("Arguments (one per line)");
  // The field says its text is stored as typed and where secrets belong.
  expect(screen.getByText(/saved and shown in plain text/u).id).toBe(
    argumentsField.getAttribute("aria-describedby"),
  );
  expect(
    screen.getByText(/saved and shown in plain text/u).textContent,
  ).toMatch(/environment variables/u);
  fireEvent.change(argumentsField, {
    target: { value: "--acp" },
  });
  fireEvent.change(
    screen.getByLabelText("Environment variable names to pass through"),
    { target: { value: "AGENT_KEY" } },
  );
  fireEvent.click(screen.getByRole("button", { name: "Save agent" }));
  await waitFor(() =>
    expect(invoke).toHaveBeenCalledWith("save_custom_acp_agent", {
      agent: {
        id: null,
        name: "Agent 1",
        executable: saved.executable,
        args: ["--acp"],
        env: ["AGENT_KEY"],
      },
    }),
  );
  expect(await screen.findByText("Agent 1")).toBeTruthy();
});

it("shows why Sage refused an agent and keeps the form open", async () => {
  invoke.mockImplementation(async (command) => {
    if (command === "custom_acp_agents") return [];
    throw new Error("The executable does not exist.");
  });
  render(<CustomAgentSettings />);
  fireEvent.click(await screen.findByRole("button", { name: "Add ACP agent" }));
  fireEvent.change(screen.getByLabelText("Name"), {
    target: { value: "Agent" },
  });
  fireEvent.change(screen.getByLabelText("Executable path"), {
    target: { value: "C:\\missing.exe" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save agent" }));
  expect(
    await screen.findByText("The executable does not exist."),
  ).toBeTruthy();
  expect(screen.getByLabelText("Name")).toBeTruthy();
});

it("stops offering more agents at the limit and removes one", async () => {
  const four = [1, 2, 3, 4].map(agent);
  invoke.mockImplementation(async (command) =>
    command === "custom_acp_agents" ? four : four.slice(1),
  );
  render(<CustomAgentSettings />);
  expect(await screen.findByText(/up to 4 agents/u)).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Add ACP agent" })).toBeNull();
  fireEvent.click(screen.getAllByRole("button", { name: "Remove" })[0]!);
  await waitFor(() =>
    expect(invoke).toHaveBeenCalledWith("remove_custom_acp_agent", {
      id: "custom-acp-1",
    }),
  );
  expect(
    await screen.findByRole("button", { name: "Add ACP agent" }),
  ).toBeTruthy();
});

it("offers nothing outside the desktop app", () => {
  vi.mocked(isTauriRuntime).mockReturnValue(false);
  const { container } = render(<CustomAgentSettings />);
  expect(container.textContent).toBe("");
  expect(invoke).not.toHaveBeenCalled();
});
