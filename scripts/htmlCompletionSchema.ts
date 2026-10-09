import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import path from "node:path";
import MagicString from "magic-string";
import type { Plugin } from "vite";

const HTML_VERSION = "6.4.12";
const HTML_SOURCE_SHA256 =
  "6f608aaeb92179d873e487060d006a5859fa382de6ceb827935bb9a1f724191f";
const SCHEMA_START = "const Targets =";
const SCHEMA_END = "Schema.default = /*@__PURE__*/new Schema;";
const EVENTS_START = "const eventAttributes =";
const EVENTS_END = "for (let a of eventAttributes)";

function htmlCompletionSchemaEdit(
  source: string,
  version: string,
): MagicString {
  if (version !== HTML_VERSION) {
    throw new Error(
      `HTML completion schema version drift: expected ${HTML_VERSION}, got ${version}`,
    );
  }
  if (
    createHash("sha256").update(source).digest("hex") !== HTML_SOURCE_SHA256
  ) {
    throw new Error(
      "HTML completion schema content drift: review the pinned upstream source before updating the guard",
    );
  }
  const start = source.indexOf(SCHEMA_START);
  const end = source.indexOf(SCHEMA_END) + SCHEMA_END.length;
  const eventsStart = source.indexOf(EVENTS_START);
  const eventsEnd = source.indexOf(EVENTS_END);
  return new MagicString(source)
    .move(eventsStart, eventsEnd, start)
    .appendRight(start, "const Schema = /*@__PURE__*/(() => {\n")
    .appendLeft(end, "\nreturn Schema;\n})();");
}

export function isolateHtmlCompletionSchema(
  source: string,
  version: string,
): string {
  return htmlCompletionSchemaEdit(source, version).toString();
}

export function htmlCompletionSchemaPlugin(): Plugin {
  let transformCount = 0;
  return {
    name: "sage-html-completion-schema",
    apply: "build",
    enforce: "pre",
    buildStart() {
      transformCount = 0;
    },
    transform(source, id) {
      if (
        !/\/node_modules\/@codemirror\/lang-html\/dist\/index\.js$/.test(
          id.replaceAll("\\", "/"),
        )
      )
        return;
      const { version } = JSON.parse(
        readFileSync(path.resolve(path.dirname(id), "../package.json"), "utf8"),
      ) as { version: string };
      const edit = htmlCompletionSchemaEdit(source, version);
      transformCount += 1;
      return {
        code: edit.toString(),
        map: edit.generateMap({
          source: id.replaceAll("\\", "/"),
          includeContent: true,
          hires: true,
        }),
      };
    },
    generateBundle() {
      if (transformCount !== 1) {
        this.error(
          `HTML completion schema transform must apply exactly once per build; got ${transformCount}`,
        );
      }
    },
  };
}
