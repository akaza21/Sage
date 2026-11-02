/* global window, document, ResizeObserver, MutationObserver, requestAnimationFrame, devicePixelRatio, getComputedStyle */
// This adapter is installed only on the configured undecorated main window.
// The measured element reserves space; real native BUTTONs own caption input
// and remain available while the WebView has a document-modal dialog open.
(() => {
  const invoke = (layout) =>
    window.__TAURI_INTERNALS__.invoke("set_window_caption_controls", {
      layout,
    });
  const host = document.createElement("sage-window-controls");
  host.setAttribute("aria-hidden", "true");
  const style = document.createElement("style");
  style.textContent = /* SAGE_WINDOW_FRAME_STYLE */ "";
  host.attachShadow({ mode: "closed" }).append(style);
  const chromeStyle = document.createElement("style");
  chromeStyle.textContent =
    ".title-bar[data-sage-window-frame] { padding-right: var(--sage-window-controls-width); }";
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = 1;
  const context = canvas.getContext("2d", { willReadFrequently: true });
  const color = (value, background) => {
    context.clearRect(0, 0, 1, 1);
    context.fillStyle = background;
    context.fillRect(0, 0, 1, 1);
    context.fillStyle = value;
    context.fillRect(0, 0, 1, 1);
    const [r, g, b] = context.getImageData(0, 0, 1, 1).data;
    return r | (g << 8) | (b << 16);
  };
  let header;
  let running = false;
  let pending = false;
  let lastLayout;
  const resize = new ResizeObserver(schedule);
  const paletteChanges = new MutationObserver(schedule);

  async function update() {
    const next = document.querySelector(".title-bar");
    if (next !== header) {
      if (header) resize.unobserve(header);
      paletteChanges.disconnect();
      header = next;
      if (header) {
        header.setAttribute("data-sage-window-frame", "");
        // Tauri's deep region excludes interactive elements and implements
        // native drag and double-click maximize/restore without another hook.
        header.setAttribute("data-tauri-drag-region", "deep");
        resize.observe(header);
        for (
          let ancestor = header;
          ancestor;
          ancestor = ancestor.parentElement
        ) {
          paletteChanges.observe(ancestor, {
            attributes: true,
            attributeFilter: ["class", "style"],
          });
        }
      }
    }
    let layout = null;
    if (header?.getClientRects().length) {
      const rect = header.getBoundingClientRect();
      // DOM rects preserve fractional CSS pixels at mixed DPI and zoom;
      // innerWidth is integer-rounded and can introduce a physical edge gap.
      const rightInset = Math.max(
        0,
        document.documentElement.getBoundingClientRect().right - rect.right,
      );
      host.style.top = `${rect.top}px`;
      host.style.right = `${rightInset}px`;
      host.style.height = `${rect.height}px`;
      const controls = host.getBoundingClientRect();
      const reservedWidth = `${controls.width}px`;
      if (
        header.style.getPropertyValue("--sage-window-controls-width") !==
        reservedWidth
      ) {
        header.style.setProperty("--sage-window-controls-width", reservedWidth);
      }
      const colors = getComputedStyle(host);
      const headerColors = getComputedStyle(header);
      const background = headerColors.backgroundColor;
      const value = (name) =>
        headerColors.getPropertyValue(name) || colors.getPropertyValue(name);
      const token = (name) => color(value(name), background);
      layout = {
        rightInset: Math.round(rightInset * devicePixelRatio),
        top: Math.round(rect.top * devicePixelRatio),
        width: Math.round(controls.width * devicePixelRatio),
        height: Math.round(rect.height * devicePixelRatio),
        palette: {
          background: color(background, background),
          foreground: token("--color-text-primary"),
          hover: token("--window-control-hover"),
          pressed: token("--window-control-pressed"),
          closeHover: token("--window-control-close-hover"),
          closePressed: token("--window-control-close-pressed"),
          closeForeground: token("--color-text-emphasis"),
          fontHeight: Math.round(
            parseFloat(value("--window-control-icon-size")) * devicePixelRatio,
          ),
        },
      };
    }
    const key = JSON.stringify(layout);
    if (key !== lastLayout) {
      lastLayout = undefined;
      await invoke(layout);
      lastLayout = key;
    }
  }
  // At most one IPC update is in flight, plus one coalesced pending update.
  // Native resize events position cached controls even when rAF is suspended.
  function schedule() {
    pending = true;
    if (running) return;
    running = true;
    requestAnimationFrame(async () => {
      try {
        do {
          pending = false;
          await update();
        } while (pending);
      } catch (error) {
        console.error(error);
      } finally {
        running = false;
        if (pending) schedule();
      }
    });
  }
  document.addEventListener("DOMContentLoaded", () => {
    // Tauri nonces bundled inline styles. Reuse that document's style nonce
    // before connecting either stylesheet, including the shadow stylesheet.
    const nonce = document.querySelector("style[nonce]")?.nonce;
    if (nonce) {
      style.nonce = nonce;
      chromeStyle.nonce = nonce;
    }
    document.head.append(chromeStyle);
    document.body.append(host);
    // Content rendering does not cause IPC work; only mounting/replacing the
    // title bar matters here. ResizeObserver owns its geometry changes.
    new MutationObserver(() => {
      if (!header?.isConnected) schedule();
    }).observe(document.getElementById("root"), {
      childList: true,
      subtree: true,
    });
    schedule();
  });
  window.addEventListener("resize", schedule);
  // A monitor-scale change can preserve the CSS viewport size, so neither
  // resize observer is guaranteed to notice its new physical pixel ratio.
  function watchScale() {
    window.matchMedia(`(resolution: ${devicePixelRatio}dppx)`).addEventListener(
      "change",
      () => {
        watchScale();
        schedule();
      },
      { once: true },
    );
  }
  watchScale();
})();
