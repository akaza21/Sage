import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import * as Lucide from "lucide-react";
import { JSDOM } from "jsdom";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import prettier from "prettier";

const names =
  `AlertTriangle Archive ArrowDown ArrowDownUp ArrowLeft ArrowRight ArrowRightLeft ArrowUp Bot Box Braces Brain BrainCircuit CaseSensitive Check ChevronDown ChevronLeft ChevronRight ChevronUp ChevronsUpDown Circle CircleAlert CirclePlus CircleX Command Copy Download Ellipsis Eye FileCode2 FileDiff FileImage FileJson FileLock FilePen FilePenLine FilePlus2 FileSearch FileSpreadsheet FileSymlink FileText Files Folder FolderInput FolderOpen FolderPlus Folders GitBranch GitCompareArrows Globe Globe2 Hand History Home Image Info Laptop Lightbulb Link2 List ListChecks ListFilter ListTree LoaderCircle Lock Maximize2 MessageCircleQuestion MessageSquare MessageSquarePlus Mic Minus NotebookText PanelLeft PanelLeftClose PanelLeftOpen PanelRightClose PanelRightOpen Paperclip Pause Pencil Pin PinOff Plus RefreshCw Regex Replace ReplaceAll RotateCcw RotateCw Search Settings ShieldCheck Sparkles Square SquareArrowOutUpRight SquareFunction SquarePen Target TerminalSquare Trash2 Unlink2 WholeWord Workflow Wrench X`.split(
    " ",
  );
// Drawn only by lazily loaded parts: they go to leanIconNodesLazy.ts, which
// the leanIcons barrel does not re-export, so startup chunks skip them.
const lazy = new Set(
  `ArrowDownUp ChevronLeft CirclePlus Download FolderInput Folders Lightbulb List ListChecks ListFilter MessageCircleQuestion PanelLeft Paperclip Pause RotateCw SquareArrowOutUpRight Target Unlink2`.split(
    " ",
  ),
);
const aliases = {
  MoreHorizontal: "Ellipsis",
  TriangleAlert: "AlertTriangle",
  XCircle: "CircleX",
};
const BARREL_FILES = 4;

function source(name) {
  const markup = renderToStaticMarkup(createElement(Lucide[name]));
  const svg = new JSDOM(markup, { contentType: "image/svg+xml" }).window
    .document.documentElement;
  const cssClass = svg.getAttribute("class").replace(/^lucide lucide-/, "");
  const nodes = [...svg.children].map((element) => {
    const attributes = Object.fromEntries(
      [...element.attributes].map((attribute) => [
        attribute.name.replace(/-([a-z])/g, (_, letter) =>
          letter.toUpperCase(),
        ),
        attribute.value,
      ]),
    );
    const keys = Object.keys(attributes);
    return element.localName === "path" && keys.length === 1 && keys[0] === "d"
      ? attributes.d
      : [element.localName, attributes];
  });
  return `export const ${name} = leanIcon(${JSON.stringify(cssClass)}, ${JSON.stringify(nodes)});`;
}

const barrel = names.filter((name) => !lazy.has(name));
const perFile = Math.ceil(barrel.length / BARREL_FILES);
const header = `import { lucideIcon as leanIcon } from "./leanIconCore";\n`;
const files = Array.from({ length: BARREL_FILES }, (_, index) => {
  const group = barrel.slice(index * perFile, (index + 1) * perFile);
  const own = Object.entries(aliases).filter(([, target]) =>
    group.includes(target),
  );
  const aliasLine = own.length
    ? `export { ${own.map(([alias, target]) => `${target} as ${alias}`).join(", ")} };\n`
    : "";
  return [
    `leanIconNodes${index + 1}.ts`,
    `${header}${group.map(source).join("\n")}\n${aliasLine}`,
  ];
});
files.push([
  "leanIconNodesLazy.ts",
  `${header}\n// Icons only lazily loaded parts draw. They are imported from here, never\n// through leanIcons, so the startup chunks that hold that barrel skip them.\n${names
    .filter((name) => lazy.has(name))
    .map(source)
    .join("\n")}\n`,
]);

const root = path.dirname(fileURLToPath(import.meta.url));
for (const [name, text] of files) {
  const file = path.resolve(root, "../src/ui", name);
  const formatted = await prettier.format(text, { filepath: file });
  if (process.argv.includes("--check")) {
    if (readFileSync(file, "utf8") !== formatted)
      throw new Error(`${file} differs from pinned Lucide geometry`);
  } else {
    writeFileSync(file, formatted);
  }
}
