import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { useRef, useState } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
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
import { useAcpChatActions } from "./useAcpChatActions";
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
    tabs: [{ id: page.chatId, kind: "chat", title: "A" }] as never,
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
  useAcpProjectionStore.getState().clear();
  useWorkbenchStore.setState(initial, true);
});

/** Real chat composer state and send/restore; buttons deliver extension offers. */
function Composer() {
  const composer = useBoundChatComposer();
  const actions = useAcpChatActions();
  const fieldRef = useRef<HTMLTextAreaElement>(null);
  const [offer, setOffer] = useState<unknown>();
  const [sessionId, setSession] = useState<string>(page.currentSessionId!);
  const [chatId, setChat] = useState<string>(page.chatId);
  const edit = (text: string) => {
    actions.suggestionRef.current = null;
    composer.setComposerText(text);
  };
  const send = (prompt: string) => {
    completion = actions.submit(prompt, [], binding, null);
  };
  return (
    <form
      onSubmit={(event) => {
        event.preventDefault();
        send(composer.composerText);
      }}
    >
      <textarea
        ref={fieldRef}
        aria-label="Message agent"
        value={composer.composerText}
        onChange={(event) => edit(event.target.value)}
      />
      <ComposerChrome
        chatAuth={{
          page: { ...page, sessionSurface: offer } as AcpChatPage,
          errorKind: null,
        }}
        dictationScope={chatId}
        suggestionRef={actions.suggestionRef}
        sessionId={sessionId}
        composerRef={fieldRef}
        actionBusy={actions.busy}
        projectTransitioning={false}
        text={composer.composerText}
        onTextChange={edit}
      />
      <button type="submit">Send</button>
      <button type="button" onClick={() => send("Original prompt")}>
        Retry failed prompt
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
      <button type="button" onClick={() => setSession("other-session")}>
        Other session
      </button>
      <button type="button" onClick={() => setSession(page.currentSessionId!)}>
        Current session
      </button>
      <button type="button" onClick={() => setChat("other-chat")}>
        Other chat
      </button>
      <button type="button" onClick={() => setChat(page.chatId)}>
        Current chat
      </button>
    </form>
  );
}

const field = () => screen.getByLabelText<HTMLTextAreaElement>("Message agent");
const click = (name: string) =>
  fireEvent.click(screen.getByRole("button", { name }));
const banner = () =>
  screen.queryByRole("button", {
    name: /Use suggestion|Replace draft with suggestion/,
  });
const finishFailure = async () =>
  act(async () => {
    fail();
    await completion;
  });

it("auto-fills only an empty current scope, announces Clear, and remembers dismissed offers", () => {
  const view = render(<Composer />);
  click("Other session");
  click("Other chat");
  click("Offer");
  expect(field().value).toBe("");
  expect(banner()).toBeTruthy();
  click("Current session");
  expect(field().value).toBe("");
  click("Current chat");
  expect(field().value).toBe("Suggested");
  expect(banner()).toBeNull();
  expect(screen.getByRole("status").textContent).toContain(
    "Suggested text added",
  );
  click("Clear suggested text");
  expect(field().value).toBe("");
  expect(banner()).toBeNull();
  view.unmount();
  offerKey++;
  render(<Composer />);
  fireEvent.change(field(), { target: { value: "My draft" } });
  click("Offer");
  expect(field().value).toBe("My draft");
  click("Ignore suggestion");
  fireEvent.change(field(), { target: { value: "" } });
  expect(field().value).toBe("");
  expect(banner()).toBeNull();
  click("Offer");
  expect(field().value).toBe("");
  expect(banner()).toBeNull();
});

it("keeps an in-flight offer as a banner and lets the failed send restore its prompt", async () => {
  render(<Composer />);
  fireEvent.change(field(), { target: { value: "Original prompt" } });
  await act(async () => {
    click("Send");
  });
  click("Offer");
  expect(field().value).toBe("");
  expect(banner()).toBeTruthy();
  await finishFailure();
  expect(field().value).toBe("Original prompt");
  expect(banner()).toBeTruthy();
  click("Replace draft with suggestion");
  expect(field().value).toBe("Suggested");
  expect(banner()).toBeNull();
});

it("restores over untouched auto-fill, but preserves a user edit even if it returns to the suggested text", async () => {
  render(<Composer />);
  click("Offer");
  expect(field().value).toBe("Suggested");
  await act(async () => {
    click("Retry failed prompt");
  });
  await finishFailure();
  expect(field().value).toBe("Original prompt");
  cleanup();
  offerKey++;
  useWorkbenchStore.getState().setComposerText("");
  render(<Composer />);
  click("Offer");
  fireEvent.change(field(), { target: { value: "My edit" } });
  fireEvent.change(field(), { target: { value: "Suggested" } });
  await act(async () => {
    click("Retry failed prompt");
  });
  await finishFailure();
  expect(field().value).toBe("Suggested");
});
