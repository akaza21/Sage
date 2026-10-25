import { visibleRectangle } from "./capture-native-chat-dom.mjs";
import { STATE_WITNESSES } from "./capture-native-editor-catalog.mjs";

export const EDITOR_MODE =
  'input[name="workbench-window-mode"][value="editor"]';
export const CONTENT = '.file-surface .cm-content[contenteditable="true"]';
export const GOTO_INPUT = 'input[aria-label="Go to line"]';

export function observeEditor(document, window, context, rectangleVisible) {
  const all = (selector) => {
    const nodes = [...document.querySelectorAll(selector)];
    if (nodes.length > 500) throw new Error("editor-dom-bound");
    return nodes;
  };
  const one = (selector) => {
    const nodes = all(selector);
    return nodes.length === 1 ? nodes[0] : null;
  };
  const css = { width: window.innerWidth, height: window.innerHeight };
  const visible = (element) => {
    if (!element || element.closest('[hidden], [aria-hidden="true"], [inert]'))
      return false;
    const style = window.getComputedStyle(element);
    return (
      style.display !== "none" &&
      style.visibility !== "hidden" &&
      style.opacity !== "0" &&
      rectangleVisible(element.getBoundingClientRect(), css)
    );
  };
  const content = one('.file-surface .cm-content[contenteditable="true"]');
  const file = one('section[aria-label="File editor ' + context.file + '"]');
  const tabs = all('.workbench__tabs [role="tab"]');
  const fileTab = tabs.find(
    (tab) =>
      tab.getAttribute("aria-selected") === "true" &&
      tab.textContent.trim() === context.file.split("/").at(-1),
  );
  const mode = one('input[name="workbench-window-mode"][value="editor"]');
  const caret = one('[title="Caret position"]')?.textContent.trim() ?? "";
  const match = /^Ln ([0-9]+), Col ([0-9]+)$/u.exec(caret);
  const caretLine = match ? Number(match[1]) : 0;
  const caretColumn = match ? Number(match[2]) : 0;
  const outline = one('section[aria-label="Outline"]');
  const symbol = context.symbolLabel
    ? all(".outline-card").find(
        (node) => node.getAttribute("aria-label") === context.symbolLabel,
      )
    : null;
  const prompt = one('[role="search"][aria-label="Go to line"]');
  const promptInput = one('input[aria-label="Go to line"]');
  const search = one(".workspace-search");
  const searchInput = one('input[aria-label="Search text"]');
  const groups = all(".workspace-search__file-group").filter(visible);
  const headings = groups.map((group) =>
    group.querySelector(".workspace-search__file-heading"),
  );
  const settings = one('section.settings-surface[aria-label="Settings"]');
  const editorPage = all('.settings-surface__nav [role="tab"]').find(
    (node) =>
      node.textContent.trim() === "Editor" &&
      node.getAttribute("aria-selected") === "true",
  );
  const witnesses = {
    file: Boolean(
      file &&
      content &&
      fileTab &&
      file.contains(content) &&
      (context.state === "outline" || (visible(content) && visible(fileTab))),
    ),
    oneTab: tabs.length === 1,
    caretMidFile:
      caretLine === context.midLine &&
      caretColumn === 1 &&
      visible(content) &&
      Boolean(
        window.getSelection()?.isCollapsed &&
        content?.contains(window.getSelection()?.anchorNode),
      ),
    editorFocused: document.activeElement === content,
    explorer:
      visible(one('section[aria-label="Explorer project tools"]')) &&
      one('.activity-rail button[aria-label="Explorer"]')?.getAttribute(
        "aria-expanded",
      ) === "true",
    outlineExpanded:
      visible(outline) &&
      one('.activity-rail button[aria-label="Outline"]')?.getAttribute(
        "aria-expanded",
      ) === "true" &&
      all(".outline-card").some(visible),
    symbolFocused: visible(symbol) && document.activeElement === symbol,
    symbolCaret: Boolean(
      context.symbolLine > 0 &&
      caretLine === context.symbolLine &&
      caretColumn === context.symbolColumn,
    ),
    prompt: visible(prompt) && promptInput?.value === String(context.midLine),
    promptFocused: document.activeElement === promptInput,
    promptStatus:
      visible(prompt?.querySelector('[role="status"]')) &&
      prompt?.querySelector('[role="status"]')?.textContent.trim() ===
        "Line or line:column",
    search:
      visible(search) &&
      one('.activity-rail button[aria-label="Search"]')?.getAttribute(
        "aria-expanded",
      ) === "true",
    searchQuery:
      searchInput?.value === "export" &&
      !one(".workspace-search__progress") &&
      !one(".workspace-search__error"),
    multipleFiles:
      new Set(
        headings
          .filter(visible)
          .map((node) =>
            node
              .getAttribute("aria-label")
              ?.replace(/^(?:Expand|Collapse) results for /u, ""),
          )
          .filter(Boolean),
      ).size >= 2,
    expandedResults: groups.some(
      (group) =>
        group.querySelector(
          '.workspace-search__file-heading[aria-expanded="true"]',
        ) &&
        [...group.querySelectorAll(".workspace-search__result")].some(visible),
    ),
    settings:
      visible(settings) && settings?.getAttribute("aria-busy") === "false",
    settingsTab:
      tabs.length === 2 &&
      tabs.filter(
        (tab) =>
          tab.getAttribute("aria-selected") === "true" &&
          tab.textContent.trim() === "Settings",
      ).length === 1,
    editorPage: visible(editorPage),
    settingsControls:
      visible(one("#editor-font-size")) &&
      !one("#editor-font-size")?.disabled &&
      all(".settings-surface__group").filter(visible).length > 0,
  };
  const predicates = {
    binding: true,
    editorMode: Boolean(mode?.checked),
    noAgentChrome:
      all(".agent-chat-pane, .project-chat-sidebar, .project-sidebar")
        .length === 0,
  };
  for (const name of context.witnesses)
    predicates[name] = witnesses[name] === true;
  predicates.namedState = Object.values(predicates).every(Boolean);
  const elements = [];
  for (const [kind, element] of [
    ["file", file],
    ["caret", one('[title="Caret position"]')],
    ["outline", outline],
    ["symbol", symbol],
    ["prompt", prompt],
    ["search", search],
    ["settings", settings],
  ]) {
    if (!visible(element)) continue;
    const { x, y, width, height } = element.getBoundingClientRect();
    const text =
      kind === "file"
        ? context.file
        : kind === "caret"
          ? caret
          : kind === "symbol"
            ? context.symbolLabel
            : kind;
    elements.push({ kind, text: text.slice(0, 128), x, y, width, height });
  }
  return {
    css,
    dpr: window.devicePixelRatio,
    predicates,
    elements,
    controls: {
      explorer: witnesses.explorer,
      outline: witnesses.outlineExpanded,
      prompt: visible(prompt),
      search: visible(search),
      settings: visible(settings),
    },
    overflow: { horizontal: document.documentElement.scrollWidth > css.width },
    lazyFallbackVisible: all(
      '.resource-state--loading, .resource-state--error, .file-surface [role="alert"]',
    ).some(visible),
  };
}
export function editorObservation(context) {
  const witnesses = STATE_WITNESSES[context.state];
  if (!witnesses) throw new Error("truthful-absent-state");
  return `(${observeEditor.toString()})(document,window,${JSON.stringify({ ...context, witnesses })},${visibleRectangle.toString()})`;
}
export function assertEditorState(observation) {
  if (
    !observation ||
    observation.lazyFallbackVisible ||
    observation.predicates?.namedState !== true ||
    Object.values(observation.predicates).some((flag) => flag !== true)
  )
    throw new Error("editor-state-drift");
  return observation;
}
