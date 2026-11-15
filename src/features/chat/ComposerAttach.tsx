import { useEffect, useId, useRef, useState, type RefObject } from "react";

import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import { isTauriRuntime } from "../../ports/nativeInvoke";
import { MenuPopover } from "../../ui/MenuPopover";
import type { ImageAction } from "./useComposerImageInput";

/**
 * Native admission treats an absent capability as unsupported; a chat without
 * a session yet is checked again when its first message starts one.
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

export function ComposerAttach({
  chatId,
  presses,
  triggerRef,
  imageAction,
  onPick,
}: {
  chatId: string;
  /** Presses of the attach button so far; each is taken once. */
  presses: number;
  /** Set before this part mounts, so the menu is placed from its first commit. */
  triggerRef: RefObject<HTMLButtonElement | null>;
  imageAction?: ImageAction;
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
