import { readFile } from "node:fs/promises";
import { expect, it } from "vitest";

it("centres the narrow composer-adjacent pill without a second scrollbar reservation", async () => {
  const source = await readFile("src/styles/chat-surface.css", "utf8");
  const narrow = source.match(
    /@container chat \(max-width: 520px\)\s*\{([\s\S]*?)\n\}/,
  )?.[1];
  expect(narrow).toBeDefined();
  expect(narrow).toMatch(
    /\.chat-edit-summary-zone\s*\{\s*padding-inline:\s*var\(--chat-column-gutter\);/,
  );
});

it("keeps narrow file rows and collapsed pill counts on one line", async () => {
  const style = document.createElement("style");
  style.textContent = (
    await readFile("src/styles/changed-files-summary.css", "utf8")
  ).replace(/@container chat \(max-width: 400px\)\s*\{([\s\S]*?)\n\}/g, "$1");
  const summary = document.createElement("div");
  summary.className = "acp-file-changes";
  summary.innerHTML =
    '<button class="acp-file-changes__header"><span class="acp-file-changes__totals"></span></button><span class="chat-file-chip"><span class="chat-file-chip__name">file.tsx</span><span class="chat-file-chip__root">Sage</span><span class="tab__count">+1</span><span class="tab__count">-1</span></span>';
  document.head.append(style);
  document.body.append(summary);
  try {
    const row = summary.querySelector(".chat-file-chip")!;
    expect(getComputedStyle(row).gridTemplateColumns).toBe(
      "auto minmax(48px, 1fr) minmax(0, auto) auto auto",
    );
    for (const detail of row.querySelectorAll(
      ".chat-file-chip__root, .tab__count",
    ))
      expect(getComputedStyle(detail).gridColumn).not.toBe("2");
    expect(
      getComputedStyle(row.querySelector(".chat-file-chip__root")!).maxWidth,
    ).toBe("64px");
    expect(
      getComputedStyle(row.querySelector(".chat-file-chip__root")!).minWidth,
    ).toBe("0px");
    const totals = summary.querySelector(".acp-file-changes__totals")!;
    expect(getComputedStyle(totals).flexWrap).not.toBe("wrap");
    summary.classList.add("acp-file-changes--collapsed");
    expect(getComputedStyle(totals).flexWrap).not.toBe("wrap");
    expect(getComputedStyle(totals).whiteSpace).toBe("nowrap");
    expect(
      getComputedStyle(summary.querySelector("button")!).gridTemplateColumns,
    ).toBe("18px minmax(0, 1fr) auto");
    const legacy = document.createElement("div");
    legacy.className = "edit-summary";
    legacy.innerHTML =
      '<div class="edit-summary__header"><span class="edit-summary__totals"></span></div><button class="edit-file"><span class="edit-file__path">file.tsx</span><span class="edit-file__counts">+1 -1</span></button>';
    summary.append(legacy);
    expect(
      getComputedStyle(legacy.querySelector(".edit-summary__header")!)
        .gridTemplateColumns,
    ).toBe("18px minmax(0, 1fr) auto");
    expect(
      getComputedStyle(legacy.querySelector(".edit-summary__totals")!).flexWrap,
    ).not.toBe("wrap");
    expect(
      getComputedStyle(legacy.querySelector(".edit-file")!).gridTemplateColumns,
    ).toBe("minmax(0, 1fr) auto auto");
  } finally {
    summary.remove();
    style.remove();
  }
});

it("drops root labels before file names or counts in the smallest chat lanes", async () => {
  const source = await readFile("src/styles/changed-files-summary.css", "utf8");
  const narrow = source.match(
    /@container chat \(max-width: 280px\)\s*\{([\s\S]*?)\n\}/,
  )?.[1];
  expect(narrow).toBeDefined();
  const style = document.createElement("style");
  style.textContent = narrow!;
  const summary = document.createElement("div");
  summary.className = "acp-file-changes";
  summary.innerHTML =
    '<span class="chat-file-chip"><span class="chat-file-chip__name">file.tsx</span><span class="chat-file-chip__root">Long project root</span><span class="tab__count">+100</span><span class="tab__count">-200</span></span>';
  document.head.append(style);
  document.body.append(summary);
  try {
    expect(
      getComputedStyle(summary.querySelector(".chat-file-chip")!)
        .gridTemplateColumns,
    ).toBe("auto minmax(48px, 1fr) auto auto");
    expect(
      getComputedStyle(summary.querySelector(".chat-file-chip__root")!).display,
    ).toBe("none");
    for (const detail of summary.querySelectorAll(
      ".chat-file-chip__name, .tab__count",
    ))
      expect(getComputedStyle(detail).display).not.toBe("none");
  } finally {
    summary.remove();
    style.remove();
  }
});
