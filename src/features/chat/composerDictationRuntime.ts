import { MAX_CHAT_COMPOSER_BYTES } from "../../domain/chatOwnership";
import { utf8ByteLength } from "../../domain/text";
import type { invokeNative } from "../../ports/nativeInvoke";

interface VoiceEvent {
  id: string;
  kind: "partial" | "final" | "ended" | "error";
  text?: string;
  message?: string;
}

export interface ActiveDictation {
  id: string;
  base: string;
  start: number;
  end: number;
  unlisten?: () => void;
}

export async function startNativeDictation(
  current: ActiveDictation,
  input: HTMLTextAreaElement | null,
  callbacks: {
    isCurrent: () => boolean;
    invoke: typeof invokeNative;
    onText: (text: string) => boolean;
    onError: (message: string) => void;
    onEnded: () => void;
  },
) {
  const { id } = current;
  const { listen } = await import("@tauri-apps/api/event");
  if (!callbacks.isCurrent()) return;
  current.unlisten = await listen<VoiceEvent>("voice-input", (event) => {
    const payload = event.payload;
    if (!callbacks.isCurrent() || payload.id !== id) return;
    if (payload.kind === "error") {
      callbacks.onError(
        payload.message ?? "Dictation failed. Check microphone access.",
      );
      return;
    }
    if (payload.kind === "ended") {
      current.unlisten?.();
      callbacks.onEnded();
      return;
    }
    const transcript = payload.text?.trim();
    if (!transcript) return;
    const prefix = current.base.slice(0, current.start);
    const suffix = current.base.slice(current.end);
    const leading = prefix && !/\s$/.test(prefix) ? " " : "";
    const trailing = suffix && !/^\s/.test(suffix) ? " " : "";
    const nextText = `${prefix}${leading}${transcript}${trailing}${suffix}`;
    if (!callbacks.onText(nextText)) return;
    const caret = prefix.length + leading.length + transcript.length;
    queueMicrotask(() => input?.setSelectionRange(caret, caret));
    if (payload.kind === "final") {
      current.base = nextText;
      current.start = caret;
      current.end = caret;
    }
  });
  if (!callbacks.isCurrent()) {
    current.unlisten();
    return;
  }
  await callbacks.invoke("start_voice_input", { id });
  if (!callbacks.isCurrent()) {
    await callbacks.invoke("stop_voice_input", { id, cancel: true });
    return;
  }
}

export interface BrowserRecognition {
  continuous: boolean;
  interimResults: boolean;
  lang: string;
  onresult:
    | ((event: {
        resultIndex: number;
        results: ArrayLike<{
          isFinal: boolean;
          [index: number]: { transcript: string };
        }>;
      }) => void)
    | null;
  onerror: (() => void) | null;
  onend: (() => void) | null;
  start: () => void;
  stop: () => void;
  abort: () => void;
}

export function startBrowserDictation(
  next: BrowserRecognition,
  callbacks: {
    isCurrent: () => boolean;
    getText: () => string;
    onText: (text: string) => boolean;
    onError: () => void;
    onEnded: () => void;
  },
) {
  next.continuous = false;
  next.interimResults = false;
  next.lang = navigator.language;
  next.onresult = (event) => {
    if (!callbacks.isCurrent()) return;
    const spoken = Array.from(event.results)
      .slice(event.resultIndex)
      .filter((result) => result.isFinal)
      .map((result) => result[0]?.transcript ?? "")
      .join(" ")
      .trim();
    if (!spoken) return;
    const base = callbacks.getText();
    callbacks.onText(`${base}${base && !/\s$/.test(base) ? " " : ""}${spoken}`);
  };
  next.onerror = () => {
    if (callbacks.isCurrent()) callbacks.onError();
  };
  next.onend = () => {
    if (callbacks.isCurrent()) callbacks.onEnded();
  };
  next.start();
}

export function createDictation(options: {
  sequence: number;
  text: string;
  input: HTMLTextAreaElement | null;
  getText: () => string;
  invoke: typeof invokeNative;
  onText: (text: string) => void;
  onError: (message: string) => void;
  onEnded: () => void;
}) {
  const commitText = (text: string) => {
    if (utf8ByteLength(text) > MAX_CHAT_COMPOSER_BYTES) {
      options.onError("Dictation is too long for this message.");
      return false;
    }
    options.onText(text);
    return true;
  };
  let active = true;
  let browser: BrowserRecognition | null = null;
  const current: ActiveDictation = {
    id:
      globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${options.sequence}`,
    base: options.text,
    start: options.input?.selectionStart ?? options.text.length,
    end: options.input?.selectionEnd ?? options.text.length,
  };
  const close = (cancel: boolean, onStopError?: (message: string) => void) => {
    active = false;
    current.unlisten?.();
    if (browser) {
      if (cancel) browser.abort();
      else browser.stop();
      browser = null;
    } else {
      void options
        .invoke("stop_voice_input", { id: current.id, cancel })
        .catch(() => {
          if (!cancel) onStopError?.("Dictation could not stop.");
        });
    }
  };
  return {
    cancel: () => close(true),
    stop: (onError: (message: string) => void) => close(false, onError),
    start: async (Constructor: (new () => BrowserRecognition) | null) => {
      if (Constructor) {
        browser = new Constructor();
        startBrowserDictation(browser, {
          isCurrent: () => active,
          getText: options.getText,
          onText: commitText,
          onError: () =>
            options.onError(
              "Dictation failed. Check microphone access and try again.",
            ),
          onEnded: () => {
            active = false;
            browser = null;
            options.onEnded();
          },
        });
      } else
        await startNativeDictation(current, options.input, {
          isCurrent: () => active,
          invoke: options.invoke,
          onText: commitText,
          onError: options.onError,
          onEnded: () => {
            active = false;
            options.onEnded();
          },
        });
      return active;
    },
  };
}
