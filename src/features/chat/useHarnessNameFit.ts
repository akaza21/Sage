import { useLayoutEffect, type RefObject } from "react";

/**
 * The harness control shows its whole name or only its brand mark, never a
 * truncated name. Whenever the composer resizes or the toolbar's text
 * changes, the name is laid out in full and, if the toolbar has no room for
 * it, hidden. Both steps run before paint, so a clipped name is never shown.
 */
export function useHarnessNameFit(control: RefObject<HTMLElement | null>) {
  useLayoutEffect(() => {
    const host = control.current;
    const composer = host?.closest(".composer");
    const toolbar = host?.closest(".composer__controls");
    if (!host || !composer || !toolbar || typeof ResizeObserver === "undefined")
      return;
    const fit = () => {
      host.removeAttribute("data-name-hidden");
      const name = host.querySelector(".searchable-picker__trigger-label");
      if (name && name.scrollWidth > name.clientWidth)
        host.setAttribute("data-name-hidden", "");
    };
    fit();
    const resize = new ResizeObserver(fit);
    resize.observe(composer);
    const text = new MutationObserver(fit);
    text.observe(toolbar, {
      subtree: true,
      childList: true,
      characterData: true,
    });
    return () => {
      resize.disconnect();
      text.disconnect();
    };
  }, [control]);
}
