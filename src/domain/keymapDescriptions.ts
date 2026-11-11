/**
 * What each shortcut does, for the Keyboard settings page and its search.
 * Kept apart from `keymapRegistry` so this copy stays out of the startup
 * bundle: matching a chord never needs it.
 */
export const KEYMAP_DESCRIPTIONS: Readonly<Record<string, string>> = {
  "agent.changes": "Open changes for the focused chat.",
  "agent.files": "Open files for the focused chat.",
  "agent.sideChat": "Open side chat for the focused chat.",
  "agent.terminal":
    "Shows or hides the terminal dock without ending its sessions.",
  newTask: "Starts a new chat and switches to Agents.",
  toggleSidebar: "Shows or hides the current mode's sidebar.",
  openSettings: "Opens settings for the current mode.",
  chatSearch: "Focuses the title bar chat search.",
  "chat.findInChat": "Search the visible chat's messages.",
  focusComposer: "Moves the caret into the message composer.",
  sendMessage: "Sends the composer contents to the agent.",
  newline: "Adds a new line in the composer instead of sending.",
  save: "Writes the active editor buffer to disk.",
  saveAll: "Saves every open file with unsaved changes.",
  find: "Opens the find bar for the active file.",
  replace: "Opens the find bar with its replace row.",
  findNext: "Moves to the next match of the find bar's text.",
  findPrevious: "Moves to the previous match of the find bar's text.",
  goToLine: "Jumps to a line number in the active file.",
  quickOpen: "Opens a file in the project by typing part of its name.",
  commandPalette: "Lists the editor's commands to run one by name.",
  navigateBack:
    "Returns to the previous chat in Agent, or the previous place in the editor.",
  navigateForward: "Undoes a Go back.",
  goToSymbol: "Jumps to a symbol defined in the active file.",
  dismiss: "Closes the preview, go to line, or find bar.",
  commit: "Submits the commit message in Source Control.",
};
