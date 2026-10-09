import { readFileSync } from "node:fs";
import path from "node:path";
import { gzipSync } from "node:zlib";
import { build } from "vite";
import { describe, expect, it } from "vitest";
import {
  htmlCompletionSchemaPlugin,
  isolateHtmlCompletionSchema,
} from "./htmlCompletionSchema";

const source = readFileSync(
  "node_modules/@codemirror/lang-html/dist/index.js",
  "utf8",
);
const version = "6.4.12";

async function languageProbe(completions = false, transform = true) {
  const result = await build({
    configFile: false,
    logLevel: "silent",
    plugins: [
      ...(transform ? [htmlCompletionSchemaPlugin()] : []),
      {
        name: "html-language-probe",
        resolveId: (id) =>
          id === "language-probe" ? "\0language-probe" : undefined,
        load: (id) =>
          id === "\0language-probe"
            ? `
          export { loadHtmlEditorLanguage } from "/src/features/files/editorLanguageHtml.ts";
          export { loadMarkdownEditorLanguage } from "/src/features/files/editorLanguageMarkdown.ts";
          export { EditorState } from "@codemirror/state";
          export { EditorView } from "@codemirror/view";
          export { ensureSyntaxTree } from "@codemirror/language";
          ${completions ? 'export { htmlCompletionSource, htmlCompletionSourceWith } from "@codemirror/lang-html";' : ""}
        `
            : undefined,
      },
    ],
    build: {
      write: false,
      target: "es2022",
      minify: true,
      rolldownOptions: {
        input: "language-probe",
        preserveEntrySignatures: "strict",
        output: { codeSplitting: false },
      },
    },
  });
  const output = Array.isArray(result) ? result[0]! : result;
  if (!("output" in output)) throw new Error("Expected probe chunks");
  const chunk = output.output.find((asset) => asset.type === "chunk");
  if (!chunk || chunk.type !== "chunk") throw new Error("Missing probe chunk");
  const probe = await import(
    /* @vite-ignore */ `data:text/javascript;base64,${Buffer.from(chunk.code).toString("base64")}`
  );
  return { code: chunk.code, probe };
}

describe("guarded HTML completion schema", () => {
  it("fails production builds when the HTML entry is renamed", async () => {
    const renamedId = path
      .resolve("node_modules/@codemirror/lang-html/dist/renamed.js")
      .replaceAll("\\", "/");
    await expect(
      build({
        configFile: false,
        logLevel: "silent",
        plugins: [
          htmlCompletionSchemaPlugin(),
          {
            name: "renamed-html-entry-probe",
            resolveId: (id) => (id === renamedId ? renamedId : undefined),
            load: (id) => (id === renamedId ? source : undefined),
          },
        ],
        build: {
          write: false,
          rolldownOptions: { input: renamedId },
        },
      }),
    ).rejects.toThrow(
      /HTML completion schema transform must apply exactly once.*got 0/,
    );
  });

  it("applies only to the pinned HTML ESM entry during builds", async () => {
    const plugin = htmlCompletionSchemaPlugin();
    expect(plugin.apply).toBe("build");
    const transform = plugin.transform;
    if (typeof transform !== "function")
      throw new Error("Expected an HTML transform hook");
    for (const id of [
      "src/features/files/editorLanguageHtml.ts",
      "node_modules/@codemirror/lang-html/dist/index.cjs",
      "node_modules/@codemirror/lang-markdown/dist/index.js",
      "node_modules/@codemirror/lang-html/dist/index.js?raw",
    ]) {
      expect(await transform.call({} as never, source, id)).toBeUndefined();
    }
    const id = path.resolve("node_modules/@codemirror/lang-html/dist/index.js");
    expect(await transform.call({} as never, source, id)).toEqual({
      code: isolateHtmlCompletionSchema(source, version),
      map: null,
    });
  });

  it("cannot tree-shake the schema through the public language-only imports alone", async () => {
    const packageMetadata = JSON.parse(
      readFileSync("node_modules/@codemirror/lang-html/package.json", "utf8"),
    );
    expect(packageMetadata.exports).toEqual({
      import: "./dist/index.js",
      require: "./dist/index.cjs",
    });
    expect(packageMetadata.sideEffects).toBe(false);
    const original = await languageProbe(false, false);
    const isolated = await languageProbe();
    expect(original.code).toContain("aria-activedescendant");
    expect(isolated.code).not.toContain("aria-activedescendant");
    expect(Object.keys(isolated.probe)).toEqual(Object.keys(original.probe));
    expect(
      gzipSync(original.code).length - gzipSync(isolated.code).length,
    ).toBeGreaterThan(2500);
  }, 30_000);

  it("fails closed on version, content, or schema-boundary drift", () => {
    expect(() => isolateHtmlCompletionSchema(source, "6.4.13")).toThrow(
      /version/,
    );
    expect(() => isolateHtmlCompletionSchema(`${source}\n`, version)).toThrow(
      /content/,
    );
    expect(() =>
      isolateHtmlCompletionSchema(
        source.replace("class Schema", "class ChangedSchema"),
        version,
      ),
    ).toThrow(/content/);
    const transformed = isolateHtmlCompletionSchema(source, version);
    expect(transformed).toContain("const Schema = /*@__PURE__*/(() => {");
    expect(
      transformed.slice(0, transformed.indexOf("const eventAttributes")),
    ).toBe(source.slice(0, source.indexOf("const Targets")));
    expect(
      transformed.slice(
        transformed.indexOf("const eventAttributes"),
        transformed.indexOf("const Schema ="),
      ),
    ).toBe(
      source.slice(
        source.indexOf("const eventAttributes"),
        source.indexOf("for (let a of eventAttributes)"),
      ),
    );
    expect(transformed.slice(transformed.indexOf("function elementName"))).toBe(
      source.slice(source.indexOf("function elementName")),
    );
  });

  it("drops unused completion tables while preserving built HTML and Markdown languages", async () => {
    const { code, probe } = await languageProbe();
    expect(code).not.toContain("aria-activedescendant");
    for (const [load, prefix] of [
      [probe.loadHtmlEditorLanguage, ""],
      [probe.loadMarkdownEditorLanguage, "# Heading\n\n"],
    ] as const) {
      const state = probe.EditorState.create({
        doc: `${prefix}<div onclick="const attrAnswer = 1;" style="background: red"></span><script>const answer = 1;</script><style>.card { color: red; }</style>`,
        extensions: [load()],
      });
      const tree = probe.ensureSyntaxTree(state, state.doc.length, 10_000)!;
      const names: string[] = [];
      tree.iterate({
        enter: (node: { name: string }) => names.push(node.name),
      });
      expect(names).toEqual(
        expect.arrayContaining(
          prefix
            ? ["ATXHeading1", "HTMLBlock"]
            : ["StartCloseTag", "VariableDeclaration", "Declaration"],
        ),
      );
      expect(names).not.toContain("MismatchedCloseTag");
      expect(state.languageDataAt("autocomplete", prefix.length)).toEqual([]);
      expect(
        tree.resolveInner(state.doc.toString().indexOf("attrAnswer") + 1).name,
      ).toBe("VariableDefinition");
      expect(
        tree.resolveInner(state.doc.toString().indexOf("background") + 1).name,
      ).toBe("PropertyName");
      expect(
        tree.resolveInner(state.doc.toString().indexOf("answer =") + 1).name,
      ).toBe("VariableDefinition");
      expect(
        tree.resolveInner(state.doc.toString().indexOf("color:") + 1).name,
      ).toBe("PropertyName");
      expect(
        tree.resolveInner(state.doc.toString().indexOf("</span>") + 3).name,
      ).toBe("TagName");
      if (prefix) expect(names).toContain("ATXHeading1");
    }
    for (const [load, doc, text, expected] of [
      [probe.loadHtmlEditorLanguage, "<section", ">", "<section>"],
      [
        probe.loadMarkdownEditorLanguage,
        "<section",
        ">",
        "<section></section>",
      ],
      [probe.loadHtmlEditorLanguage, "<section>body<", "/", "<section>body</"],
      [
        probe.loadMarkdownEditorLanguage,
        "<section>body<",
        "/",
        "<section>body</section>",
      ],
      [probe.loadHtmlEditorLanguage, "<br", ">", "<br>"],
      [probe.loadMarkdownEditorLanguage, "<br", ">", "<br>"],
    ] as const) {
      const parent = document.createElement("div");
      document.body.append(parent);
      const view = new probe.EditorView({
        parent,
        state: probe.EditorState.create({
          doc,
          extensions: [load()],
        }),
      });
      try {
        view.dispatch({ selection: { anchor: view.state.doc.length } });
        view.contentDOM.dispatchEvent(
          new InputEvent("beforeinput", {
            bubbles: true,
            cancelable: true,
            data: text,
            inputType: "insertText",
          }),
        );
        view.contentDOM.append(document.createTextNode(text));
        view.observer.flush();
        expect(view.state.doc.toString()).toBe(expected);
      } finally {
        view.destroy();
        parent.remove();
      }
    }
  }, 30_000);

  it("retains upstream default and custom completion behavior when explicitly requested", async () => {
    const { probe } = await languageProbe(true);
    const state = probe.EditorState.create({
      doc: "<",
      extensions: [probe.loadHtmlEditorLanguage()],
    });
    const context = { state, pos: 1, explicit: true };
    expect(probe.htmlCompletionSource(context).options).toEqual(
      expect.arrayContaining([expect.objectContaining({ label: "section" })]),
    );
    expect(
      probe.htmlCompletionSourceWith({ extraTags: { "sage-widget": {} } })(
        context,
      ).options,
    ).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ label: "sage-widget" }),
      ]),
    );
    for (const [doc, label] of [
      ["<section aria-", "aria-label"],
      ["<section on", "onclick"],
    ]) {
      const state = probe.EditorState.create({
        doc,
        extensions: [probe.loadHtmlEditorLanguage()],
      });
      expect(
        probe.htmlCompletionSource({ state, pos: doc.length, explicit: true })
          .options,
      ).toEqual(expect.arrayContaining([expect.objectContaining({ label })]));
    }
  }, 30_000);
});
