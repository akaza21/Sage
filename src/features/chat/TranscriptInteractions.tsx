import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";
import { useStore } from "zustand";
import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../app/store";
import {
  attachedContextKey,
  MAX_ATTACHED_CONTEXTS_PER_TURN,
  type AttachedContext,
} from "../../domain/attachedContext";
import {
  isTranscriptQuote,
  type TranscriptQuote,
  type TranscriptQuoteContext,
} from "../../domain/transcriptQuote";
import { openSideChat } from "../../workbench/sideChatActions";
import { AnnotationComment } from "./AnnotationComment";
import { TranscriptAnnotations } from "./TranscriptAnnotations";
import { TranscriptSelectionPopover } from "./TranscriptSelectionPopover";
import {
  quoteContext,
  requestAnnotation,
  useAnnotationRequest,
} from "./transcriptSelection";
import "./transcript-interactions.css";

export { ComposerAnnotations, SentAnnotations } from "./AnnotationChip";
export { openSideChat };
export { readTranscriptQuote } from "./transcriptSelection";
export {
  SideChatDialogs,
  SideChatHeader,
  SideChatHome,
} from "../../workbench/SideChatChrome";

const EMPTY: readonly AttachedContext[] = [];
const STATUS_MS = 4000;

export function TranscriptInteractions({
  contentRef,
  ...props
}: {
  chatId: string;
  focused: boolean;
  contentRef: RefObject<HTMLElement | null>;
  initialKeyboard: boolean;
}) {
  const [elements, setElements] = useState<{
    content: HTMLElement;
    scrollport: HTMLElement;
  } | null>(null);
  useLayoutEffect(() => {
    const content = contentRef.current;
    const scrollport = content?.parentElement;
    if (content && scrollport) setElements({ content, scrollport });
  }, [contentRef]);
  return elements ? <Interactions {...props} {...elements} /> : null;
}

function Interactions({
  chatId,
  focused,
  content,
  scrollport,
  initialKeyboard,
}: {
  chatId: string;
  focused: boolean;
  content: HTMLElement;
  scrollport: HTMLElement;
  initialKeyboard: boolean;
}) {
  const contexts = useWorkbenchStore(
    (state) => state.chatUiById[chatId]?.attachedContext ?? EMPTY,
  );
  const busy = useWorkbenchStore(
    (state) =>
      state.projectTransitioning || Boolean(state.chatUiById[chatId]?.sending),
  );
  const [editingKey, setEditingKey] = useState<string | null>(null);
  const [status, setStatus] = useState("");
  const keyboardOnce = useRef(initialKeyboard);
  const takeInitialKeyboard = useCallback(() => {
    const keyboard = keyboardOnce.current;
    keyboardOnce.current = false;
    return keyboard;
  }, []);
  useAnnotationRequest("edit", chatId, setEditingKey);
  useEffect(() => {
    if (!status) return;
    const timer = setTimeout(() => setStatus(""), STATUS_MS);
    return () => clearTimeout(timer);
  }, [status]);
  const quotes = useMemo(() => contexts.filter(isTranscriptQuote), [contexts]);
  const page = useStore(acpProjectionStoreFor(chatId), (state) => state.page);
  const isForeign = useCallback(
    (quote: TranscriptQuoteContext) =>
      page?.nextCursor === null &&
      !page.turns.some((turn) => turn.id === quote.sourceTurnId),
    [page],
  );
  const editing = quotes.find(
    (quote) => attachedContextKey(quote) === editingKey,
  );
  const attach = (context: TranscriptQuoteContext) => {
    const state = useWorkbenchStore.getState();
    const intent = state.captureChatOwnership(chatId);
    return Boolean(intent && state.attachChatContext(intent, context));
  };
  const add = (quote: TranscriptQuote) => {
    const context = { ...quoteContext(quote), ordinal: quotes.length + 1 };
    const key = attachedContextKey(context);
    const prior = contexts.find(
      (candidate) => attachedContextKey(candidate) === key,
    );
    if (!prior && contexts.length >= MAX_ATTACHED_CONTEXTS_PER_TURN) {
      setStatus("Up to 8 annotations per message");
      return;
    }
    if (prior || attach(context)) setEditingKey(key);
  };
  const sideChat = (quote: TranscriptQuote) => {
    openSideChat(chatId, quote).catch((error: unknown) =>
      setStatus(
        error instanceof Error ? error.message : "Side chat could not open",
      ),
    );
  };
  const bounds = status ? scrollport.getBoundingClientRect() : null;
  return createPortal(
    <>
      {bounds ? (
        <p
          className="transcript-selection-status"
          role="status"
          style={{ left: bounds.left + bounds.width / 2, top: bounds.bottom }}
        >
          {status}
        </p>
      ) : null}
      {focused && !busy && !editing ? (
        <TranscriptSelectionPopover
          content={content}
          scrollport={scrollport}
          takeInitialKeyboard={takeInitialKeyboard}
          onAdd={add}
          onSideChat={sideChat}
        />
      ) : null}
      <TranscriptAnnotations
        content={content}
        scrollport={scrollport}
        quotes={quotes}
        editingKey={editing ? editingKey : null}
        isForeign={isForeign}
        onOpen={() => requestAnnotation("show", chatId)}
      />
      {editing && !busy ? (
        <AnnotationComment
          key={editingKey}
          quote={editing}
          active={focused}
          content={content}
          scrollport={scrollport}
          onSave={(comment, restoreFocus) => {
            const { comment: _previous, ...rest } = editing;
            attach(comment.trim() ? { ...rest, comment } : rest);
            setEditingKey(null);
            if (restoreFocus)
              document.getElementById(`prompt-${chatId}`)?.focus();
          }}
        />
      ) : null}
    </>,
    document.body,
  );
}
