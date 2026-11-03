import { useEffect, useId, useRef, useState } from "react";
import { useStore } from "zustand";
import { acpProjectionStoreFor } from "../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../app/store";
import { ModalDialog } from "../ui/ModalDialog";
import { leanIcon } from "../ui/leanIconCore";
import { X } from "../ui/leanIcons";
import { acpDesktop } from "../services/acpDesktop";
import {
  cancelCloseSideChat,
  closeSideChat,
  requestCloseSideChat,
} from "./sideChatActions";
import {
  MAX_SIDE_CHATS,
  useSideChatState,
  type SideChatEntry,
} from "./sideChatState";
import { useAcpChatLiveSnapshot } from "../features/chat/acp/useAcpChatLiveSnapshot";
import { invokeNative, isTauriRuntime } from "../ports/nativeInvoke";
import "../features/chat/transcript-interactions.css";

const MessageCirclePlus = leanIcon("lucide lucide-message-circle-plus", [
  [
    "path",
    {
      d: "M2.992 16.342a2 2 0 0 1 .094 1.167l-1.065 3.29a1 1 0 0 0 1.236 1.168l3.413-.998a2 2 0 0 1 1.099.092 10 10 0 1 0-4.777-4.719",
    },
  ],
  ["path", { d: "M8 12h8" }],
  ["path", { d: "M12 8v8" }],
]);

/** Focus the side composer once, when it first becomes enabled after open. */
const FOCUS_WAIT_MS = 5000;

export function SideChatHeader({
  chatId,
  running,
}: {
  chatId: string;
  running: boolean;
}) {
  const [error, setError] = useState("");
  const prompt = useStore(acpProjectionStoreFor(chatId), (state) => {
    const item = state.page?.turns[0]?.items.find(
      (candidate) =>
        candidate.kind === "text" && candidate.payload.role === "user",
    );
    return item?.kind === "text" ? item.payload.text.slice(0, 128) : null;
  });
  useEffect(() => {
    // Tab lives outside its pane, in the center header.
    const pane = document.querySelector(
      `.agent-chat-pane[data-chat-id="${CSS.escape(chatId)}"]`,
    );
    if (!pane) return;
    const focus = () => {
      const composer = pane.querySelector<HTMLTextAreaElement>("textarea");
      if (!composer || composer.disabled) return false;
      composer.focus();
      return true;
    };
    if (focus()) return;
    const observer = new MutationObserver(() => {
      if (focus()) observer.disconnect();
    });
    observer.observe(pane, {
      subtree: true,
      childList: true,
      attributes: true,
      attributeFilter: ["disabled"],
    });
    const timer = setTimeout(() => observer.disconnect(), FOCUS_WAIT_MS);
    return () => {
      clearTimeout(timer);
      observer.disconnect();
    };
  }, [chatId]);
  const title = prompt?.trim() || "Side chat";
  return (
    <div className="agent-chat-pane__identity side-chat-tab">
      <span className="side-chat-tab__icon" aria-hidden="true">
        {running ? (
          // Use the shared 12px spinner; .spinner alone is unstyled.
          <span className="spinner agent-chat-chip__spinner" />
        ) : (
          <MessageCirclePlus size={14} />
        )}
      </span>
      <strong className="agent-chat-pane__title" title={title}>
        {title}
      </strong>
      <span className="sr-only">(temporary)</span>
      <button
        type="button"
        className="side-chat-tab__close"
        aria-label="Close side chat"
        onClick={() => {
          setError("");
          requestCloseSideChat(chatId).catch((reason: unknown) =>
            setError(
              reason instanceof Error
                ? reason.message
                : "Side chat could not be closed",
            ),
          );
        }}
      >
        <X size={12} strokeWidth={2.625} aria-hidden="true" />
      </button>
      {error ? (
        <span className="side-chat-tab__error" role="alert">
          {error}
        </span>
      ) : null}
    </div>
  );
}

/** Mount registry/running sync while any side chat exists. */
export function SideChatDialogs() {
  useSideChatRegistry();
  const pending = useSideChatState((state) => state.pendingClose);
  const projectId = useWorkbenchStore((state) => state.project?.id);
  const side = useSideChatState((state) =>
    state.sides.find((entry) => entry.id === pending),
  );
  const current = useSideChatState((state) =>
    state.sides.find((entry) => entry.projectId === projectId),
  );
  const snapshot = useAcpChatLiveSnapshot(current?.id ?? "", Boolean(current));
  useEffect(() => {
    // Trust Rust's running flag; fixtures derive it.
    if (isTauriRuntime() || !current || current.running === snapshot.working)
      return;
    useSideChatState.setState((state) => ({
      sides: state.sides.map((entry) =>
        entry.id === current.id
          ? { ...entry, running: snapshot.working }
          : entry,
      ),
    }));
  }, [current, snapshot.working]);
  return pending && side?.projectId === projectId ? (
    <CloseSideChatDialog
      key={pending}
      onCancel={cancelCloseSideChat}
      onClose={(checked) => closeSideChat(pending, checked)}
    />
  ) : null;
}

export function SideChatHome() {
  return (
    <div className="side-chat-home">
      <MessageCirclePlus size={24} aria-hidden="true" />
      <strong>Side chat</strong>
      <p>Side chats are temporary and disappear when you close the app.</p>
    </div>
  );
}

/** Close dialog initially focuses the danger action. */
export function CloseSideChatDialog({
  onCancel,
  onClose,
}: {
  onCancel: () => void;
  onClose: (dontAskAgain: boolean) => Promise<void>;
}) {
  const title = useId();
  const danger = useRef<HTMLButtonElement>(null);
  const busyRef = useRef(false);
  const [checked, setChecked] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    const previous =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    danger.current?.focus();
    return () => {
      if (previous?.isConnected) previous.focus();
    };
  }, []);
  const dismiss = () => {
    if (!busyRef.current) onCancel();
  };
  return (
    <ModalDialog
      className="close-side-chat-dialog dialog--scrim"
      labelledBy={title}
      onDismiss={dismiss}
    >
      <h2 id={title}>Close side chat?</h2>
      <p>
        This side chat will be gone and can&apos;t be recovered. Are you sure?
      </p>
      <label>
        <input
          type="checkbox"
          checked={checked}
          disabled={busy}
          onChange={(event) => setChecked(event.target.checked)}
        />{" "}
        Don&apos;t ask again
      </label>
      {error ? <p role="alert">{error}</p> : null}
      <div className="dirty-dialog__actions">
        <button
          type="button"
          className="button"
          disabled={busy}
          onClick={dismiss}
        >
          Cancel
        </button>
        <button
          ref={danger}
          type="button"
          className="button button--danger"
          disabled={busy}
          onClick={() => {
            if (busyRef.current) return;
            busyRef.current = true;
            setBusy(true);
            setError("");
            void onClose(checked)
              .catch((error: unknown) => {
                setError(
                  error instanceof Error
                    ? error.message
                    : "Side chat could not be closed",
                );
              })
              .finally(() => {
                busyRef.current = false;
                setBusy(false);
              });
          }}
        >
          Close side chat
        </button>
      </div>
    </ModalDialog>
  );
}

/** Refresh Rust's temporary Chat mirror on side-chat projection changes. */
function useSideChatRegistry() {
  const epoch = useWorkbenchStore((state) => state.nativeTransitionEpoch);
  useEffect(() => {
    if (!isTauriRuntime()) return;
    let disposed = false;
    let pending = false;
    let dirty = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let unlisten: (() => void) | undefined;
    const refresh = async () => {
      if (disposed) return;
      if (pending) {
        dirty = true;
        return;
      }
      pending = true;
      try {
        const rows = await invokeNative<SideChatEntry[]>("list_side_chats");
        if (
          !disposed &&
          Array.isArray(rows) &&
          rows.length <= MAX_SIDE_CHATS &&
          rows.every(
            (row) =>
              [row.id, row.projectId, row.parentChatId].every(
                (id) => typeof id === "string" && id.length <= 512,
              ) &&
              row.id.startsWith("side-") &&
              typeof row.running === "boolean",
          )
        )
          useSideChatState.setState({ sides: rows });
      } finally {
        pending = false;
        if (!disposed && dirty) {
          dirty = false;
          schedule();
        }
      }
    };
    const schedule = () => {
      if (!timer)
        timer = setTimeout(() => {
          timer = null;
          void refresh().catch(() => undefined);
        }, 50);
    };
    void acpDesktop
      .subscribeProjectionChanges((event) => {
        if (
          useSideChatState
            .getState()
            .sides.some((side) => side.id === event.chatId)
        )
          schedule();
      })
      .then((dispose) => {
        if (disposed) dispose();
        else {
          unlisten = dispose;
          schedule();
        }
      })
      .catch(schedule);
    return () => {
      disposed = true;
      if (timer) clearTimeout(timer);
      unlisten?.();
    };
  }, [epoch]);
}
