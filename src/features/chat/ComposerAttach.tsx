import { useId, useMemo, useState } from "react";

import type { PromptPick } from "../../ports/promptAttachments";
import { MenuPopover } from "../../ui/MenuPopover";
import { attachRefusal as refusal } from "./composerAttachRefusal";
import type { useComposerImageInput } from "./useComposerImageInput";

/** The composer's image action, offered as the attach menu's Images item. */
export type ComposerImageAction = NonNullable<
  ReturnType<typeof useComposerImageInput>["action"]
>;

const PICKS = [
  ["files", "Files…"],
  ["folder", "Folder…"],
] as const satisfies readonly (readonly [PromptPick, string])[];

/**
 * Lazy: loads with the first Files and folders press. Says why files
 * cannot be attached here, and offers native's file or folder dialog, and
 * images when the harness takes images. The picked files join the chat's
 * one staged batch, which the paste part stages, shows and sends, so
 * pasted text and picked files go with the same message.
 */
export function ComposerAttach({
  chatId,
  presses,
  triggerId,
  imageAction,
  onPick,
}: {
  chatId: string;
  /** Presses of Files and folders so far; each is taken once. */
  presses: number;
  /** The control the menu anchors on. */
  triggerId: string;
  imageAction?: ComposerImageAction;
  /** Asks the batch's owner for native's dialog. */
  onPick: (pick: PromptPick) => void;
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
  // Each press is taken once: it opens the menu, or says why nothing here
  // can be attached.
  const [taken, setTaken] = useState(0);
  if (presses !== taken) {
    setTaken(presses);
    const refused = refusal(chatId);
    if (refused && !imageAction) setNotice(refused);
    else setMenuOpen(true);
  }
  // The anchor belongs to the toolbar, so it is looked up when the menu
  // places itself rather than when this part renders.
  const triggerRef = useMemo(
    () => ({
      get current() {
        return document.getElementById(triggerId);
      },
    }),
    [triggerId],
  );
  const refused = menuOpen ? refusal(chatId) : null;
  const close = () => {
    setMenuOpen(false);
    triggerRef.current?.focus();
  };
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
          {PICKS.map(([pick, label]) => (
            <button
              key={pick}
              type="button"
              role="menuitem"
              className="menu-item"
              tabIndex={-1}
              disabled={refused !== null}
              title={refused ?? undefined}
              onClick={() => {
                close();
                onPick(pick);
              }}
            >
              {label}
            </button>
          ))}
          {imageAction ? (
            <button
              type="button"
              role="menuitem"
              className="menu-item"
              tabIndex={-1}
              disabled={imageAction.disabled}
              onClick={() => {
                close();
                imageAction.onChoose();
              }}
            >
              Images…
            </button>
          ) : null}
        </MenuPopover>
      ) : null}
    </>
  );
}
