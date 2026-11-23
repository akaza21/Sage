import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useRef, useState } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type {
  AcpChatPage,
  ClientIntentId,
  CuratedBindingId,
  CuratedModelId,
  ProjectGeneration,
  ProjectionRevision,
  RuntimeEpoch,
  SageAgentSessionId,
  SageChatId,
  SageProjectId,
} from "../../domain/acp";
import { agentSessionFixture } from "../../domain/acpProjection.fixture";
import type { AcpCommandList } from "../../services/acpCommandsDesktop";
import {
  createAcpActionController,
  type AcpActionCommands,
} from "./acp/acpActionController";
import { ComposerTerminalCatch } from "./ComposerTerminalCatch";

afterEach(cleanup);

const scope = {
  projectId: "project-a" as SageProjectId,
  projectGeneration: "3" as ProjectGeneration,
  chatId: "chat-a" as SageChatId,
};
const bindingId = "binding-a" as CuratedBindingId;
const modelId = "model-a" as CuratedModelId;
const page: AcpChatPage = {
  ...scope,
  title: "Chat",
  projectionRevision: "10" as ProjectionRevision,
  currentSessionId: "session-a" as SageAgentSessionId,
  sessions: [
    agentSessionFixture({
      id: "session-a" as SageAgentSessionId,
      bindingId,
      modelId,
      state: "active",
      residency: "hot",
      runtimeEpoch: "2" as RuntimeEpoch,
    }),
  ],
  turns: [],
  nextCursor: null,
};
// The native list as `list_acp_commands` returns it for a curated binding.
const list: AcpCommandList = {
  revision: 7,
  commands: [
    { name: "review", description: "", inputHint: null, source: "harness" },
  ],
  terminalHandoff: {
    label: "Example",
    command: 'agent --resume "native-1"',
    terminalOnly: ["hooks"],
  },
};

function composer(initial: string) {
  let intent = 0;
  // The access check refuses a send before it reaches the send route.
  const access = { settled: true, checks: 0 };
  const sendTurn = vi.fn(async (_request: { prompt: string }) => ({
    projection: page,
  }));
  const runCommand = vi.fn(async () => ({ projection: page }));
  const controller = createAcpActionController({
    commands: { sendTurn } as unknown as AcpActionCommands,
    slashCommands: { listCommands: async () => list, runCommand },
    getContext: () => ({
      scope,
      ownershipEpoch: 1,
      commandProjectGeneration: "11" as ProjectGeneration,
      page,
    }),
    applyAuthoritativePage: () => "accepted",
    settleAccessMode: async () => {
      access.checks += 1;
      return access.settled;
    },
    createIntentId: () => `intent-${++intent}` as ClientIntentId,
  });
  function Composer() {
    const fieldRef = useRef<HTMLTextAreaElement>(null);
    const [text, setText] = useState(initial);
    const [status, setStatus] = useState("");
    return (
      <form
        aria-label="Composer"
        onSubmit={(event) => {
          event.preventDefault();
          void controller.submit(text, [], bindingId, modelId);
        }}
      >
        <textarea
          aria-label="Message"
          ref={fieldRef}
          value={text}
          onChange={(event) => setText(event.target.value)}
        />
        <ComposerTerminalCatch
          chatId="chat-a"
          fieldRef={fieldRef}
          onAnnounce={setStatus}
          text={text}
        />
        {/* The field's status region, mounted ahead of any catch. */}
        <span role="status">{status}</span>
      </form>
    );
  }
  render(<Composer />);
  return { sendTurn, runCommand, access };
}

const HELD =
  "/hooks only runs in the terminal. Copy the command to resume this chat there, then type /hooks.";
const submit = () => fireEvent.submit(screen.getByRole("form"));
const prompts = (sendTurn: { mock: { calls: [{ prompt: string }][] } }) =>
  sendTurn.mock.calls.map(([request]) => request.prompt);

it("holds a terminal-only command for a choice, copies the terminal command and sends as text once", async () => {
  const writeText = vi.fn(async (_text: string) => undefined);
  Object.assign(navigator, { clipboard: { writeText } });
  const sent = composer("/hooks");

  submit();
  await vi.waitFor(() =>
    expect(screen.getByRole("status").textContent).toBe(HELD),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Copy terminal command" }),
  );
  expect(writeText).toHaveBeenCalledWith('agent --resume "native-1"');
  expect(screen.getByRole<HTMLTextAreaElement>("textbox").value).toBe("/hooks");
  expect(sent.sendTurn).not.toHaveBeenCalled();
  expect(sent.runCommand).not.toHaveBeenCalled();

  fireEvent.click(screen.getByRole("button", { name: "Send as text anyway" }));
  await vi.waitFor(() => expect(prompts(sent.sendTurn)).toEqual(["/hooks"]));
  expect(sent.runCommand).not.toHaveBeenCalled();
});

it("asks again after one consented send and releases the choice on edit", async () => {
  const sent = composer("/hooks");
  submit();
  fireEvent.click(
    await screen.findByRole("button", { name: "Send as text anyway" }),
  );
  await vi.waitFor(() => expect(sent.sendTurn).toHaveBeenCalledTimes(1));
  fireEvent.change(screen.getByRole("textbox"), {
    target: { value: "/hooks" },
  });
  submit();
  await screen.findByRole("group", { name: "Terminal-only command" });
  expect(sent.sendTurn).toHaveBeenCalledTimes(1);
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "/hook" } });
  expect(
    screen.queryByRole("group", { name: "Terminal-only command" }),
  ).toBeNull();
  expect(screen.getByRole("status").textContent).toBe("");
});

it("disarms an unused consent when the text changes, so the command asks again", async () => {
  const sent = composer("/hooks");
  submit();
  await screen.findByRole("group", { name: "Terminal-only command" });
  // The consented send is refused before it reaches the send route.
  sent.access.settled = false;
  fireEvent.click(screen.getByRole("button", { name: "Send as text anyway" }));
  // Wait for the refusal to settle, so the chat's action queue is free.
  await vi.waitFor(() => expect(sent.access.checks).toBe(2));
  await new Promise((resolve) => setTimeout(resolve, 0));
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "/hook" } });
  fireEvent.change(screen.getByRole("textbox"), {
    target: { value: "/hooks" },
  });
  sent.access.settled = true;
  submit();
  await screen.findByRole("group", { name: "Terminal-only command" });
  expect(sent.sendTurn).not.toHaveBeenCalled();
});
