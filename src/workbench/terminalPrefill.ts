/** Long enough for a first dock and shell to load; afterwards nothing is typed. */
const PREFILL_TIMEOUT_MS = 10_000;
// The dock screen's text input, where a user's own paste lands.
const TERMINAL_INPUT = ".terminal-dock__screen textarea";

/**
 * Opens the chat's terminal dock on a new tab and pastes `text` there for the
 * user to run: typed, never run, through the dock's own input path. It drives
 * the dock as a user would, so the terminal chunk carries no handoff code.
 * One observer, released on the paste or the timeout.
 */
export function prefillTerminal(text: string, open: () => void) {
  const known = new Set(document.querySelectorAll(TERMINAL_INPUT));
  open();
  const paste = (input: Element) => {
    const data = new DataTransfer();
    data.setData("text/plain", text);
    input.dispatchEvent(
      new ClipboardEvent("paste", { clipboardData: data, bubbles: true }),
    );
  };
  const observer = new MutationObserver(() => {
    const input = [...document.querySelectorAll(TERMINAL_INPUT)].find(
      (candidate) => !known.has(candidate),
    );
    if (!input) return;
    stop();
    paste(input);
  });
  const timer = setTimeout(() => stop(), PREFILL_TIMEOUT_MS);
  function stop() {
    observer.disconnect();
    clearTimeout(timer);
  }
  observer.observe(document.body, { childList: true, subtree: true });
  // A dock that already has tabs gets a new one; a fresh dock creates its own.
  if (known.size)
    requestAnimationFrame(() =>
      document
        .querySelector<HTMLButtonElement>(
          ".terminal-dock [aria-label='New terminal']:not(:disabled)",
        )
        ?.click(),
    );
}
