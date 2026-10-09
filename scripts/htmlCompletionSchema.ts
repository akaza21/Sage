import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import path from "node:path";
import type { Plugin } from "vite";

const HTML_VERSION = "6.4.12";
const HTML_SOURCE_SHA256 =
  "6f608aaeb92179d873e487060d006a5859fa382de6ceb827935bb9a1f724191f";
const SCHEMA_START = "const Targets =";
const SCHEMA_END = "Schema.default = /*@__PURE__*/new Schema;";
const EVENTS_START = "const eventAttributes =";
const EVENTS_END = "for (let a of eventAttributes)";

export function isolateHtmlCompletionSchema(
  source: string,
  version: string,
): string {
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
  return `${source.slice(0, start)}${source.slice(eventsStart, eventsEnd)}const Schema = /*@__PURE__*/(() => {\n${source.slice(start, eventsStart)}${source.slice(eventsEnd, end)}\nreturn Schema;\n})();${source.slice(end)}`;
}

export function htmlCompletionSchemaPlugin(): Plugin {
  return {
    name: "sage-html-completion-schema",
    apply: "build",
    enforce: "pre",
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
      return { code: isolateHtmlCompletionSchema(source, version), map: null };
    },
  };
}
