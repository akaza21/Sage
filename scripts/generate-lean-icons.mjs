import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import * as Lucide from "lucide-react";
import { JSDOM } from "jsdom";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import prettier from "prettier";

const names =
  `AlertTriangle Archive ArrowLeft ArrowRight ArrowRightLeft ArrowUp Bot Box Braces Brain BrainCircuit CaseSensitive Check ChevronDown ChevronLeft ChevronRight ChevronsUpDown ChevronUp Circle CircleAlert CircleX Clock3 Copy Download Ellipsis ExternalLink Eye FileCode2 FileDiff FileImage FileJson FileLock FilePen FilePenLine FilePlus2 Files FileSearch FileSpreadsheet FileSymlink FileText Folder FolderInput FolderOpen FolderPlus GitBranch GitCompareArrows Globe Globe2 History Home Image Info Laptop Link2 ListFilter ListTree LoaderCircle Maximize2 MessageCircleQuestion MessageSquare MessageSquarePlus Mic Minus MoreHorizontal NotebookText PanelLeftClose PanelLeftOpen PanelRightClose PanelRightOpen Pause Pencil Pin PinOff Plug Plus RefreshCw Regex Replace ReplaceAll RotateCcw RotateCw Search Settings ShieldCheck SlidersHorizontal Sparkles Square SquareArrowOutUpRight SquareFunction SquarePen Target Terminal TerminalSquare Trash2 TriangleAlert Unlink2 WholeWord Workflow Wrench X XCircle`.split(
    " ",
  );
const root = path.dirname(fileURLToPath(import.meta.url));
const groups = [[], [], [], []];
for (let index = 0; index < names.length; index += 1) {
  const name = names[index];
  const markup = renderToStaticMarkup(createElement(Lucide[name]));
  const svg = new JSDOM(markup, { contentType: "image/svg+xml" }).window
    .document.documentElement;
  const cssClass = svg.getAttribute("class");
  const nodes = [...svg.children].map((element) => [
    element.localName,
    Object.fromEntries(
      [...element.attributes].map((attribute) => [
        attribute.name.replace(/-([a-z])/g, (_, letter) =>
          letter.toUpperCase(),
        ),
        attribute.value,
      ]),
    ),
  ]);
  groups[Math.floor(index / 25)].push(
    `export const ${name} = leanIcon(${JSON.stringify(cssClass)}, ${JSON.stringify(nodes)});`,
  );
}
for (let index = 0; index < groups.length; index += 1) {
  const file = path.resolve(root, `../src/ui/leanIconNodes${index + 1}.ts`);
  const source = await prettier.format(
    `import { leanIcon } from "./leanIconCore";\n${groups[index].join("\n")}\n`,
    { filepath: file },
  );
  if (process.argv.includes("--check")) {
    if (readFileSync(file, "utf8") !== source)
      throw new Error(`${file} differs from pinned Lucide geometry`);
  } else {
    writeFileSync(file, source);
  }
}
