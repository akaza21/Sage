import { useId, useRef, useState, type RefObject } from "react";

import type { PromptPick } from "../../ports/promptAttachments";
import { MenuPopover } from "../../ui/MenuPopover";
import { attachRefusal as refusal } from "./composerAttachRefusal";
import { ComposerImageInput } from "./ComposerImageInput";
import type { PickedImages } from "./pastedTextTypes";
import { pickedImages } from "./pickedImages";
import { imagesOffered, type ImageAction } from "./useComposerImageInput";

const PICKS = [
  ["files", "Files…"],
  ["folder", "Folder…"],
] as const satisfies readonly (readonly [PromptPick, string])[];

/**
 * Lazy: loads with the first Attach press. Picked files join the chat's one
 * staged batch, which the paste part owns, so pasted text and picked files
 * go with the same message. Images are just files: where the harness takes
 * images, a picked PNG or JPEG goes as an image; where it takes images but
 * no files, Files… picks images only.
 */
export function ComposerAttach({
  chatId,
  presses,
  triggerRef,
  imageAction,
  onPick,
}: {
  chatId: string;
  /** Presses of Attach so far; each is taken once. */
  presses: number;
  /** The control the menu anchors on, readable from this part's first commit. */
  triggerRef: RefObject<HTMLElement | null>;
  imageAction?: ImageAction;
  onPick: (pick: PromptPick, images?: PickedImages) => void;
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
  if (presses !== taken) {
    setTaken(presses);
    const refused = refusal(chatId);
    if (refused && !imagesOffered(imageAction)) setNotice(refused);
    else setMenuOpen(true);
  }
  const refused = menuOpen ? refusal(chatId) : null;
  const images = imagesOffered(imageAction) ? imageAction : undefined;
  const chooser = useRef<HTMLInputElement>(null);
  const close = () => {
    setMenuOpen(false);
    triggerRef.current?.focus();
  };
  return (
    <>
      <p className="sr-only" role="status" aria-live="polite">
        {notice}
      </p>
      {images ? (
        <ComposerImageInput
          inputRef={chooser}
          disabled={images.disabled}
          onChoose={images.accept}
        />
      ) : null}
      {menuOpen ? (
        <MenuPopover
          id={menuId}
          className="composer__popover"
          ariaLabel="Attach"
          open
          triggerRef={triggerRef}
          sharedTrigger
          onDismiss={() => setMenuOpen(false)}
          gap={8}
        >
          {PICKS.map(([pick, label]) => {
            // Files refused, images taken: Files… chooses images only.
            const imageOnly = refused !== null && pick === "files" && images;
            return (
              <button
                key={pick}
                type="button"
                role="menuitem"
                className="menu-item"
                tabIndex={-1}
                disabled={imageOnly ? imageOnly.disabled : refused !== null}
                title={(!imageOnly && refused) || undefined}
                onClick={() => {
                  close();
                  if (imageOnly) chooser.current?.click();
                  else
                    onPick(
                      pick,
                      // Full, or busy uploading: picked images stay files.
                      images && !images.disabled
                        ? pickedImages(chatId, images.accept)
                        : undefined,
                    );
                }}
              >
                {label}
              </button>
            );
          })}
        </MenuPopover>
      ) : null}
    </>
  );
}
