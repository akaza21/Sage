import { slashTokenEnd } from "./composerSlashCommand";

// Goes through the editing commands so Undo restores what was typed.
export function insertSlashCommand(
  field: HTMLTextAreaElement,
  name: string | null,
  input = "",
) {
  const token = slashTokenEnd(field.value);
  const end = field.value.startsWith("/")
    ? token < field.value.length
      ? token + 1
      : token
    : 0;
  // A null name (a panel command) leaves nothing behind.
  const replacement =
    name === null
      ? ""
      : `/${name}${end ? field.value.charAt(token) || " " : " "}${input}${input && end < field.value.length ? " " : ""}`;
  field.focus();
  field.setSelectionRange(0, end);
  const inserted =
    typeof document.execCommand === "function" &&
    document.execCommand(
      replacement ? "insertText" : "delete",
      false,
      replacement,
    );
  if (!inserted) {
    field.setRangeText(replacement, 0, end, "end");
    field.dispatchEvent(new Event("input", { bubbles: true }));
  }
}

// A pending pick lives on its own textarea until the lazily loaded slash
// controller adopts it. One bounded descriptor; no global listener or cache.
export function pickComposerSlashCommand(
  field: HTMLTextAreaElement,
  command: { name: string; inputHint: string | null },
  input: string,
) {
  field.setAttribute(
    "data-sage-command-pick",
    JSON.stringify({ name: command.name, inputHint: command.inputHint }),
  );
  insertSlashCommand(field, command.name, input);
  field.dispatchEvent(new Event("sage-command-picked"));
}

export function takeComposerSlashPick(field: HTMLTextAreaElement) {
  const pick = field.getAttribute("data-sage-command-pick");
  field.removeAttribute("data-sage-command-pick");
  return pick
    ? (JSON.parse(pick) as { name: string; inputHint: string | null })
    : null;
}
