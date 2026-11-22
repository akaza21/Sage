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
import { AgentToolsContext } from "../../workbench/AgentToolsProvider";
import {
  createAcpActionController,
  type AcpActionCommands,
} from "./acp/acpActionController";
import { ComposerSlashController } from "./ComposerCommandRuntime";

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
    label: "Example Agent",
    command: 'agent --resume "native-1"',
    terminalOnly: [{ name: "hooks", aliases: [] }],
    controls: [],
  },
};

function sender() {
  let intent = 0;
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
    createIntentId: () => `intent-${++intent}` as ClientIntentId,
  });
  return { sendTurn, runCommand, controller };
}

function Composer({ submit }: { submit: (text: string) => void }) {
  const fieldRef = useRef<HTMLTextAreaElement>(null);
  const [text, setText] = useState("/hooks");
  const readyRef = useRef(false);
  return (
    <form
      aria-label="Composer"
      onSubmit={(event) => {
        event.preventDefault();
        submit(text);
      }}
    >
      <textarea
        id="prompt-chat-a"
        aria-label="Message"
        ref={fieldRef}
        value={text}
        onChange={(event) => setText(event.target.value)}
      />
      <ComposerSlashController
        boundChatId="chat-a"
        disabled={false}
        fieldId="prompt-chat-a"
        fieldRef={fieldRef}
        onTextChange={setText}
        readyRef={readyRef}
        text={text}
      />
    </form>
  );
}

it("offers the terminal or plain text for a terminal-only command and sends nothing until one is picked", async () => {
  const launch = vi.fn();
  const sent = sender();
  render(
    <AgentToolsContext
      value={{ launch, chatAvailable: true, terminalAvailable: true }}
    >
      <Composer
        submit={(text) =>
          void sent.controller.submit(text, [], bindingId, modelId)
        }
      />
    </AgentToolsContext>,
  );
  const submit = () => fireEvent.submit(screen.getByRole("form"));

  submit();
  await screen.findByText("/hooks needs Example Agent's terminal.");
  expect(screen.getByRole("button", { name: "Open in terminal" })).toBeTruthy();
  expect(sent.sendTurn).not.toHaveBeenCalled();
  expect(sent.runCommand).not.toHaveBeenCalled();
  expect(screen.getByRole<HTMLTextAreaElement>("textbox").value).toBe("/hooks");

  fireEvent.click(screen.getByRole("button", { name: "Send as text anyway" }));
  await vi.waitFor(() => expect(sent.sendTurn).toHaveBeenCalledTimes(1));
  expect(sent.sendTurn.mock.calls[0]![0]).toMatchObject({ prompt: "/hooks" });
  expect(sent.runCommand).not.toHaveBeenCalled();

  // Consent covers one send: the next one asks again.
  submit();
  fireEvent.click(
    await screen.findByRole("button", { name: "Open in terminal" }),
  );
  expect(launch).toHaveBeenCalledWith("terminal", "chat-a");
  expect(screen.getByRole<HTMLTextAreaElement>("textbox").value).toBe("");
  expect(screen.queryByText(/needs Example Agent's terminal/u)).toBeNull();
  expect(sent.sendTurn).toHaveBeenCalledTimes(1);
});
