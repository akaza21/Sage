import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";

import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import { isTauriRuntime } from "../../ports/nativeInvoke";
import { MenuPopover } from "../../ui/MenuPopover";
import type { ImageAction } from "./useComposerImageInput";

/**
 * Why files cannot be staged for this chat's next message, or null. Native
 * admission treats an absent capability as unsupported; a chat without a
 * session yet is checked again when its first message starts one.
 */
function refusal(chatId: string): string | null {
  if (!isTauriRuntime())
    return "Attaching files needs the Sage desktop app with an ACP harness.";
  const page = acpProjectionStoreFor(chatId).getState().page;
  const session = page?.sessions.find(
    (item) => item.id === page.currentSessionId,
  );
  return session && session.capabilities.promptResources !== true
    ? "This harness does not accept attached files (no embedded-context support)."
    : null;
}

/**
 * Lazy: loads with the first press of the attach button. Says why files
 * cannot be attached here, offers images beside files when the harness
 * takes images, and asks for native's file dialog. The picked files join
 * the chat's one staged batch, which the paste part stages, shows and
 * sends, so pasted text and picked files go with the same message.
 */
export function ComposerAttach({
  chatId,
  presses,
  triggerId,
  imageAction,
  onPick,
}: {
  chatId: string;
  /** Presses of the attach button so far; each is taken once. */
  presses: number;
  /** The attach button, which anchors the menu. */
  triggerId: string;
  imageAction?: ImageAction;
  /** Asks the batch's owner for native's file dialog. */
  onPick: () => void;
}) {
  const [notice, setNotice] = useState("");
  const [menuOpen, setMenuOpen] = useState(false);
  const menuId = useId();
  // The composer keeps this part across chats: another chat starts quiet.
  const [scope, setScope] = useState(chatId);
  if (scope !== chatId) {
    setScope(chatId);
    setNotice("");
    setMenuOpen(false);
  }
  // Each press is taken once: it opens the menu, says why files cannot be
  // attached, or asks for the native picker, which opens once per ask.
  const [taken, setTaken] = useState(0);
  const [picks, setPicks] = useState(0);
  if (presses !== taken) {
    setTaken(presses);
    const refused = refusal(chatId);
    if (imageAction && !imageAction.reason) setMenuOpen(true);
    else if (refused) setNotice(refused);
    else setPicks((count) => count + 1);
  }
  const picked = useRef(0);
  useEffect(() => {
    if (picks === picked.current) return;
    picked.current = picks;
    onPick();
  }, [onPick, picks]);
  // The menu anchors on the attach button.
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  useLayoutEffect(() => {
    triggerRef.current = document.getElementById(
      triggerId,
    ) as HTMLButtonElement | null;
  });
  const refused = menuOpen ? refusal(chatId) : null;
  return (
    <>
      <p className="sr-only" role="status" aria-live="polite">
        {notice}
      </p>
      {menuOpen ? (
        <MenuPopover
          id={menuId}
          className="composer__popover"
          ariaLabel="Attach"
          open
          triggerRef={triggerRef}
          onDismiss={() => setMenuOpen(false)}
          gap={8}
        >
          <button
            type="button"
            role="menuitem"
            className="menu-item"
            tabIndex={-1}
            disabled={refused !== null}
            title={refused ?? undefined}
            onClick={() => {
              setMenuOpen(false);
              setPicks((count) => count + 1);
            }}
          >
            Files…
          </button>
          <button
            type="button"
            role="menuitem"
            className="menu-item"
            tabIndex={-1}
            disabled={imageAction?.disabled}
            onClick={() => {
              setMenuOpen(false);
              imageAction?.onChoose();
            }}
          >
            Images…
          </button>
        </MenuPopover>
      ) : null}
    </>
  );
}
