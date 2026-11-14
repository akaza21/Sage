import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createRef } from "react";

const native = vi.hoisted(() => ({
  invoke: vi.fn(async (..._args: unknown[]) => undefined),
  listen: vi.fn(),
  emit: null as null | ((payload: unknown) => void),
}));
vi.mock("../../ports/nativeInvoke", () => ({
  isTauriRuntime: () => true,
  invokeNative: native.invoke,
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: native.listen,
}));

import { useComposerDictation } from "./composerChrome";

beforeEach(() => {
  vi.stubGlobal("SpeechRecognition", undefined);
  vi.stubGlobal("webkitSpeechRecognition", undefined);
  vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
  native.invoke.mockReset();
  native.invoke.mockResolvedValue(undefined);
  native.listen.mockImplementation(async (_name, callback) => {
    native.emit = (payload: unknown) => callback({ payload });
    return () => {
      native.emit = null;
    };
  });
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  native.emit = null;
});

describe("native composer dictation", () => {
  it("keeps its hooks stable when native speech support changes", () => {
    const platform = vi.spyOn(navigator, "platform", "get");
    const ref = createRef<HTMLTextAreaElement>();
    const { result, rerender } = renderHook(() =>
      useComposerDictation("", vi.fn(), "chat-a", false, ref),
    );
    expect(result.current.supported).toBe(true);
    platform.mockReturnValue("Linux");
    expect(() => rerender()).not.toThrow();
    expect(result.current.supported).toBe(false);
  });

  it("inserts partial then final speech at the caret without duplicating interim words", async () => {
    const input = document.createElement("textarea");
    input.value = "before after";
    input.setSelectionRange(7, 7);
    const ref = createRef<HTMLTextAreaElement>();
    Object.defineProperty(ref, "current", { value: input });
    let text = "before after";
    const change = vi.fn((value: string) => {
      text = value;
      input.value = value;
    });
    const { result } = renderHook(() =>
      useComposerDictation(text, change, "chat-a", false, ref),
    );
    expect(result.current.supported).toBe(true);
    await act(async () => result.current.start());
    const id = (native.invoke.mock.calls[0]?.[1] as { id: string }).id;
    expect(native.invoke).toHaveBeenCalledWith("start_voice_input", { id });
    act(() => native.emit?.({ id, kind: "partial", text: "hello" }));
    expect(text).toBe("before hello after");
    act(() => native.emit?.({ id, kind: "final", text: "hello world" }));
    expect(text).toBe("before hello world after");
    await waitFor(() => expect(input.selectionStart).toBe(18));
    act(() => result.current.stop());
    expect(native.invoke).toHaveBeenCalledWith("stop_voice_input", {
      id,
      cancel: false,
    });
    const emitLate = native.emit;
    act(() => emitLate?.({ id, kind: "final", text: "late" }));
    expect(text).toBe("before hello world after");
  });

  it("stops on window blur and ignores late speech from that recording", async () => {
    const input = document.createElement("textarea");
    const ref = createRef<HTMLTextAreaElement>();
    Object.defineProperty(ref, "current", { value: input });
    const change = vi.fn();
    const { result } = renderHook(() =>
      useComposerDictation("Draft", change, "chat-a", false, ref),
    );
    await act(async () => result.current.start());
    const id = (native.invoke.mock.calls[0]?.[1] as { id: string }).id;
    const emitLate = native.emit;
    act(() => window.dispatchEvent(new Event("blur")));
    expect(native.invoke).toHaveBeenCalledWith("stop_voice_input", {
      id,
      cancel: true,
    });
    act(() => emitLate?.({ id, kind: "final", text: "late" }));
    expect(change).not.toHaveBeenCalled();
  });

  it("keeps a newer recording alive when stopping the previous recording fails late", async () => {
    let rejectStop!: (reason: Error) => void;
    native.invoke.mockImplementation(async (command) => {
      if (command === "stop_voice_input")
        await new Promise<void>((_resolve, reject) => {
          rejectStop = reject;
        });
    });
    const input = document.createElement("textarea");
    input.value = "Draft";
    const ref = createRef<HTMLTextAreaElement>();
    Object.defineProperty(ref, "current", { value: input });
    const { result } = renderHook(() =>
      useComposerDictation(
        "Draft",
        (value) => {
          input.value = value;
        },
        "chat-a",
        false,
        ref,
      ),
    );
    await act(async () => result.current.start());
    act(() => result.current.stop());
    await act(async () => result.current.start());
    const calls = native.invoke.mock.calls.filter(
      ([command]) => command === "start_voice_input",
    );
    const id = (calls.at(-1)?.[1] as { id: string }).id;
    await act(async () => rejectStop(new Error("Late stop failure")));
    expect(result.current.listening).toBe(true);
    act(() => native.emit?.({ id, kind: "final", text: "still recording" }));
    expect(input.value).toBe("Draft still recording");
  });

  it("surfaces a native microphone failure without inserting text", async () => {
    const ref = createRef<HTMLTextAreaElement>();
    const change = vi.fn();
    const { result } = renderHook(() =>
      useComposerDictation("Draft", change, "chat-a", false, ref),
    );
    await act(async () => result.current.start());
    const id = (native.invoke.mock.calls[0]?.[1] as { id: string }).id;
    act(() =>
      native.emit?.({
        id,
        kind: "error",
        message:
          "Check microphone access and Online speech recognition settings.",
      }),
    );
    expect(result.current.error).toMatch(/microphone access/);
    expect(result.current.listening).toBe(false);
    expect(change).not.toHaveBeenCalled();
  });
});
