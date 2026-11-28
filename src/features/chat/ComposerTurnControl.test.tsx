import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { useRef, useState } from "react";
import { afterEach, expect, it, vi } from "vitest";
import { ComposerTurnControl } from "./ComposerTurnControl";
import { useComposerDictation } from "./composerChrome";
import type { BrowserRecognition } from "./composerDictationRuntime";
import type * as NativeInvoke from "../../ports/nativeInvoke";

const native = vi.hoisted(() => ({
  enabled: false,
  id: "",
  emit: null as null | ((payload: unknown) => void),
}));
vi.mock("../../ports/nativeInvoke", async (importOriginal) => {
  const actual = await importOriginal<typeof NativeInvoke>();
  return {
    ...actual,
    isTauriRuntime: () => native.enabled,
    invokeNative: async (_command: string, args: { id: string }) => {
      native.id = args.id;
    },
  };
});
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (
    _name: string,
    callback: (event: { payload: unknown }) => void,
  ) => {
    native.emit = (payload) => callback({ payload });
    return () => {
      native.emit = null;
    };
  },
}));

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  native.enabled = false;
  native.emit = null;
});

class Speech implements BrowserRecognition {
  static latest: Speech;
  static starts = 0;
  continuous = false;
  interimResults = false;
  lang = "";
  onresult: BrowserRecognition["onresult"] = null;
  onerror: BrowserRecognition["onerror"] = null;
  onend: BrowserRecognition["onend"] = null;
  constructor() {
    Speech.latest = this;
  }
  start() {
    Speech.starts += 1;
  }
  stop() {
    this.onend?.();
  }
  abort() {
    this.onend?.();
  }
  emit(text: string) {
    this.onresult?.({
      resultIndex: 0,
      results: [{ isFinal: true, 0: { transcript: text } }],
    });
  }
}

it("keeps dictation on one control until stopped and disables it during the turn", async () => {
  vi.stubGlobal("SpeechRecognition", Speech);
  let dictation!: ReturnType<typeof useComposerDictation>;
  function Composer() {
    const input = useRef<HTMLTextAreaElement>(null);
    const [text, setText] = useState("");
    const [activeTurn, setActiveTurn] = useState(false);
    dictation = useComposerDictation(text, setText, "chat", activeTurn, input);
    return (
      <form
        onSubmit={(event) => {
          event.preventDefault();
          setActiveTurn(true);
        }}
      >
        <textarea
          ref={input}
          aria-label="Prompt"
          value={text}
          onChange={(event) => setText(event.target.value)}
        />
        <ComposerTurnControl
          dictation={dictation}
          activeTurn={activeTurn}
          hasContent={Boolean(text.trim())}
          onCancelTurn={() => setActiveTurn(false)}
          sendDisabled={!text.trim()}
        />
      </form>
    );
  }
  render(<Composer />);
  let control = screen.getByRole("button", { name: "Start dictation" });
  expect(control).toBeEnabled();
  fireEvent.click(control);
  await waitFor(() => expect(dictation.listening).toBe(true));
  act(() => Speech.latest.emit("Review the composer"));
  const input = screen.getByRole("textbox", { name: "Prompt" });
  expect(input).toHaveValue("Review the composer");
  expect(screen.getByRole("button", { name: "Stop dictation" })).toBe(control);
  expect(control).toHaveAttribute("aria-pressed", "true");
  fireEvent.click(control);
  expect(input).toHaveValue("Review the composer");
  fireEvent.click(screen.getByRole("button", { name: "Send message" }));
  // Stop joins Send and the muted microphone instead of replacing them.
  expect(
    screen.getAllByRole("button").map((button) => button.ariaLabel),
  ).toEqual(["Start dictation", "Send message", "Cancel turn"]);
  expect(dictation.disabled).toBe(true);
  expect(control).toBeDisabled();
  expect(control).toHaveAttribute("aria-pressed", "false");
  control = screen.getByRole("button", { name: "Cancel turn" });
  await act(async () => {
    await dictation.start();
  });
  act(() => Speech.latest.emit("late speech"));
  expect(input).toHaveValue("Review the composer");
  fireEvent.click(control);
  expect(screen.queryByRole("button", { name: "Cancel turn" })).toBeNull();
  expect(screen.getByRole("button", { name: "Send message" })).toBeEnabled();
  fireEvent.change(input, { target: { value: "" } });
  expect(screen.getByRole("button", { name: "Start dictation" })).toBeEnabled();
  expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
});

it("keeps a native partial speech draft when stopped and sends it only on Send", async () => {
  native.enabled = true;
  vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
  function Composer() {
    const input = useRef<HTMLTextAreaElement>(null);
    const [text, setText] = useState("");
    const [sent, setSent] = useState("");
    const dictation = useComposerDictation(text, setText, "chat", false, input);
    return (
      <form
        onSubmit={(event) => {
          event.preventDefault();
          setSent(text);
          setText("");
        }}
      >
        <textarea ref={input} aria-label="Prompt" value={text} readOnly />
        <output aria-label="Sent prompt">{sent}</output>
        <ComposerTurnControl
          dictation={dictation}
          activeTurn={false}
          hasContent={Boolean(text.trim())}
          onCancelTurn={null}
          sendDisabled={!text.trim()}
        />
      </form>
    );
  }
  render(<Composer />);
  const control = screen.getByRole("button", { name: "Start dictation" });
  fireEvent.click(control);
  await screen.findByRole("button", { name: "Stop dictation" });
  act(() =>
    native.emit?.({
      id: native.id,
      kind: "partial",
      text: "Review the composer",
    }),
  );
  const input = screen.getByRole("textbox", { name: "Prompt" });
  expect(input).toHaveValue("Review the composer");
  expect(screen.getByRole("button", { name: "Stop dictation" })).toBe(control);
  expect(control).toHaveClass("composer__mic");
  expect(control).toHaveAttribute("aria-pressed", "true");
  fireEvent.click(control);
  expect(input).toHaveValue("Review the composer");
  expect(
    screen.getByRole("status", { name: "Sent prompt" }),
  ).toBeEmptyDOMElement();
  const send = screen.getByRole("button", { name: "Send message" });
  expect(send).not.toHaveAttribute("aria-pressed");
  fireEvent.click(send);
  expect(screen.getByRole("status", { name: "Sent prompt" })).toHaveTextContent(
    "Review the composer",
  );
  expect(input).toHaveValue("");
});

it("never restarts dictation from a Stop press that outlived the recording", async () => {
  vi.stubGlobal("SpeechRecognition", Speech);
  Speech.starts = 0;
  function Composer() {
    const input = useRef<HTMLTextAreaElement>(null);
    const [text, setText] = useState("");
    const dictation = useComposerDictation(text, setText, "chat", false, input);
    return (
      <ComposerTurnControl
        dictation={dictation}
        activeTurn={false}
        hasContent={Boolean(text.trim())}
        sendDisabled={!text.trim()}
      />
    );
  }
  render(<Composer />);
  fireEvent.click(screen.getByRole("button", { name: "Start dictation" }));
  const stop = await screen.findByRole("button", { name: "Stop dictation" });
  fireEvent.pointerDown(stop);
  act(() => Speech.latest.onend?.());
  expect(stop).toHaveAccessibleName("Start dictation");
  fireEvent.click(stop);
  await act(async () => {});
  expect(stop).toHaveAccessibleName("Start dictation");
  expect(Speech.starts).toBe(1);
  // Held Space: recording ends mid-hold and the repeat must not forget Stop.
  fireEvent.click(stop);
  await screen.findByRole("button", { name: "Stop dictation" });
  fireEvent.keyDown(stop, { key: " " });
  act(() => Speech.latest.onend?.());
  fireEvent.keyDown(stop, { key: " ", repeat: true });
  fireEvent.click(stop);
  await act(async () => {});
  expect([stop.getAttribute("aria-label"), Speech.starts]).toEqual([
    "Start dictation",
    2,
  ]);
  // Shift never activates the button, so a later screen-reader click starts.
  fireEvent.click(stop);
  await screen.findByRole("button", { name: "Stop dictation" });
  fireEvent.keyDown(stop, { key: "Shift" });
  act(() => Speech.latest.onend?.());
  fireEvent.click(stop);
  await screen.findByRole("button", { name: "Stop dictation" });
  expect(Speech.starts).toBe(4);
});

it("keeps Send for typed text while an uncancellable turn runs and queues it", () => {
  function Composer() {
    const [text, setText] = useState("");
    const [queued, setQueued] = useState<string[]>([]);
    const input = useRef<HTMLTextAreaElement>(null);
    const dictation = useComposerDictation(text, setText, "chat", true, input);
    return (
      <form
        onSubmit={(event) => {
          event.preventDefault();
          setQueued((rows) => [...rows, text]);
          setText("");
        }}
      >
        <textarea
          ref={input}
          aria-label="Prompt"
          value={text}
          onChange={(event) => setText(event.target.value)}
        />
        <output aria-label="Queued">{queued.join("|")}</output>
        <ComposerTurnControl
          dictation={dictation}
          activeTurn
          hasContent={Boolean(text.trim())}
          onCancelTurn={null}
          sendDisabled={!text.trim()}
        />
      </form>
    );
  }
  render(<Composer />);
  expect(screen.getByRole("button", { name: "Cancel turn" })).toBeDisabled();
  fireEvent.change(screen.getByRole("textbox", { name: "Prompt" }), {
    target: { value: "Next step" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Send message" }));
  expect(screen.getByRole("status", { name: "Queued" })).toHaveTextContent(
    "Next step",
  );
  expect(screen.getByRole("button", { name: "Cancel turn" })).toBeDisabled();
});
