import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { acpProjectionStoreFor } from "../../../app/acpProjectionRegistry";
import { useAcpProjectionStore } from "../../../app/acpProjectionStore";
import { useWorkbenchStore } from "../../../app/store";
import { useSessionAccessModeStore } from "../../../app/sessionAccessModeStore";
import type {
  AcpChatPage,
  CuratedBindingId,
  ProjectGeneration,
} from "../../../domain/acp";
import { AcpDesktopError, acpDesktop } from "../../../services/acpDesktop";
import { useBoundChatComposer } from "../useBoundChatComposer";
import { autoFilled, useAcpChatActions } from "./useAcpChatActions";
import { ComposerChrome } from "./ComposerChrome";

const initial = useWorkbenchStore.getState();
const binding = "fixture" as CuratedBindingId;
let page: AcpChatPage;
let fail: () => void;
let completion: Promise<unknown>;
let offerKey = 0;

beforeEach(() => {
  useSessionAccessModeStore.setState({ ready: true, error: null });
  offerKey++;
  page = {
    projectId: "project-a",
    chatId: "chat-a",
    title: "A",
    projectionRevision: "1",
    currentSessionId: "session-a",
    nextCursor: null,
    sessions: [
      {
        id: "session-a",
        ordinal: 1,
        bindingId: binding,
        modelId: null,
        state: "active",
        residency: "hot",
        runtimeEpoch: "1",
        legacy: false,
        capabilities: { prompt: true },
      },
    ],
    turns: [],
  } as unknown as AcpChatPage;
  useWorkbenchStore.setState({
    project: { id: page.projectId, name: "A", roots: [] },
    tabs: [
      { id: page.chatId, kind: "chat", title: "A" },
      { id: "chat-b", kind: "chat", title: "B" },
    ] as never,
    activeTabId: page.chatId,
    archivedChats: [],
    chatUiById: {},
    composerText: "",
    sending: false,
    projectGeneration: 0,
    nativeTransitionEpoch: 0,
    chatOwnershipEpoch: 1,
    projectTransitioning: false,
  });
  const projection = useAcpProjectionStore.getState();
  const scope = {
    projectId: page.projectId,
    chatId: page.chatId,
    projectGeneration: "0" as ProjectGeneration,
  };
  projection.activateScope(scope);
  projection.applyAuthoritativePage(scope, page);
  vi.spyOn(acpDesktop, "sendTurn").mockImplementation(
    () =>
      new Promise((_, reject) => {
        fail = () =>
          reject(
            new AcpDesktopError({
              kind: "invalidRequest",
              message: "Try again",
            }),
          );
      }),
  );
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  autoFilled.clear();
  acpProjectionStoreFor("chat-b").getState().clear();
  useAcpProjectionStore.getState().clear();
  useWorkbenchStore.setState(initial, true);
});

/**
 * The real chat composer state and send/restore path, unbound (the active
 * chat) or bound to `chatId`. "Offer" delivers an extension's editor text;
 * "Send queued" sends a prompt that is not in the field, as the queue does.
 */
function Composer({ chatId }: { chatId?: string }) {
  const composer = useBoundChatComposer(chatId);
  const actions = useAcpChatActions(chatId);
  const [offer, setOffer] = useState<unknown>();
  const scope = chatId ?? page.chatId;
  const send = (prompt: string) => {
    completion = actions.submit(prompt, [], binding, null);
  };
  return (
    <form
      aria-label={scope}
      onSubmit={(event) => {
        event.preventDefault();
        send(composer.composerText);
      }}
    >
      <textarea
        aria-label="Message agent"
        value={composer.composerText}
        onChange={(event) => composer.setComposerText(event.target.value)}
      />
      <ComposerChrome
        chatAuth={{
          page: {
            ...page,
            chatId: scope,
            sessionSurface: offer,
          } as AcpChatPage,
          errorKind: null,
        }}
        dictationScope={scope}
        actionBusy={actions.busy}
        projectTransitioning={false}
        text={composer.composerText}
        onTextChange={composer.setComposerText}
      />
      <button type="submit">Send</button>
      <button type="button" onClick={() => send("Queued prompt")}>
        Send queued
      </button>
      <button
        type="button"
        onClick={() =>
          setOffer({
            activities: [],
            chrome: {
              editorText: { key: String(offerKey), text: "Suggested" },
            },
          })
        }
      >
        Offer
      </button>
    </form>
  );
}

const chat = (name = "chat-a") => within(screen.getByRole("form", { name }));
const field = (name?: string) =>
  chat(name).getByLabelText<HTMLTextAreaElement>("Message agent");
const click = (button: string, name?: string) =>
  fireEvent.click(chat(name).getByRole("button", { name: button }));
const banner = () =>
  screen.queryByRole("button", { name: /Use suggestion|Replace draft/ });
const failSend = async () =>
  act(async () => {
    fail();
    await completion;
  });
const sendAndFail = async (name?: string) => {
  await act(async () => click("Send queued", name));
  await failSend();
};

it("never overwrites a draft, fills an empty idle field by itself, and Clear empties it for good", () => {
  render(<Composer />);
  fireEvent.change(field(), { target: { value: "My draft" } });
  click("Offer");
  expect(field().value).toBe("My draft");
  expect(
    screen.getByRole("button", { name: "Replace draft with suggestion" }),
  ).toBeTruthy();
  click("Ignore suggestion");
  expect(banner()).toBeNull();
  fireEvent.change(field(), { target: { value: "" } });
  offerKey++;
  click("Offer");
  expect(field().value).toBe("Suggested");
  expect(banner()).toBeNull();
  expect(screen.getByRole("status").textContent).toContain(
    "Suggested text added",
  );
  click("Clear suggested text");
  expect(field().value).toBe("");
  // The answered offer does not come back.
  click("Offer");
  expect(field().value).toBe("");
  expect(banner()).toBeNull();
});

it("keeps the offer as a banner while a send is in flight, and the failed send's restore wins", async () => {
  render(<Composer />);
  fireEvent.change(field(), { target: { value: "My prompt" } });
  await act(async () => click("Send"));
  click("Offer");
  expect(field().value).toBe("");
  expect(banner()).toBeTruthy();
  await failSend();
  expect(field().value).toBe("My prompt");
  expect(banner()).toBeTruthy();
});

it("lets a failed restore replace untouched auto-filled text per chat, never edited text", async () => {
  const scopeB = {
    projectId: page.projectId,
    chatId: "chat-b" as typeof page.chatId,
    projectGeneration: "0" as ProjectGeneration,
  };
  const storeB = acpProjectionStoreFor("chat-b");
  storeB.getState().activateScope(scopeB);
  storeB.getState().applyAuthoritativePage(scopeB, {
    ...page,
    chatId: "chat-b",
  } as AcpChatPage);
  render(
    <>
      <Composer />
      <Composer chatId="chat-b" />
    </>,
  );
  click("Offer");
  click("Offer", "chat-b");
  expect(field().value).toBe("Suggested");
  expect(field("chat-b").value).toBe("Suggested");
  // Each side-by-side chat keeps its own untouched fill.
  await sendAndFail("chat-b");
  expect(field("chat-b").value).toBe("Queued prompt");
  expect(field().value).toBe("Suggested");
  await sendAndFail();
  expect(field().value).toBe("Queued prompt");
  cleanup();
  offerKey++;
  useWorkbenchStore.getState().setComposerText("");
  render(<Composer />);
  click("Offer");
  fireEvent.change(field(), { target: { value: "My edit" } });
  fireEvent.change(field(), { target: { value: "Suggested" } });
  await sendAndFail();
  expect(field().value).toBe("Suggested");
});
