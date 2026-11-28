// Reads a harness's published TypeScript declarations with the TypeScript
// compiler (already a devDependency) and turns its wire types into a
// contract graph: each object type once, as `{ field: [child type ids] }`.
import { execFileSync } from "node:child_process";
import path from "node:path";

import ts from "typescript";

import { OPAQUE } from "./shape.mjs";

// Installs one published package (no scripts, no platform binaries) into
// `work` and returns its folder. A Windows shell joins arguments unquoted,
// and the temp path may hold a space.
export function installed(spec, work) {
  const prefix = process.platform === "win32" ? `"${work}"` : work;
  execFileSync(
    "npm",
    [
      "install",
      "--ignore-scripts",
      "--omit=optional",
      "--no-audit",
      "--no-fund",
      "--prefix",
      prefix,
      spec,
    ],
    {
      shell: process.platform === "win32",
      stdio: ["ignore", "ignore", "inherit"],
    },
  );
  return path.join(work, "node_modules", spec.slice(0, spec.lastIndexOf("@")));
}

// The wire types declared in `entries`, the harness's own declaration files.
export function declarations(...entries) {
  const program = ts.createProgram(entries, {
    noEmit: true,
    skipLibCheck: true,
    strict: true,
    target: ts.ScriptTarget.ES2022,
    module: ts.ModuleKind.NodeNext,
    moduleResolution: ts.ModuleResolutionKind.NodeNext,
  });
  const checker = program.getTypeChecker();
  const files = entries.map((entry) => {
    const file = program.getSourceFile(entry);
    if (!file) throw new Error(`${entry}: not readable`);
    const unresolved = ts
      .getPreEmitDiagnostics(program, file)
      .filter((diagnostic) => diagnostic.code === 2307);
    if (unresolved.length)
      throw new Error(
        `${entry}: unresolved imports: ${unresolved
          .map((diagnostic) =>
            ts.flattenDiagnosticMessageText(diagnostic.messageText, " "),
          )
          .join("; ")}`,
      );
    return file;
  });
  // A named type alias or interface in the declaration files, exported or not.
  const find = (name) =>
    files
      .flatMap((file) => file.statements)
      .find(
        (statement) =>
          (ts.isTypeAliasDeclaration(statement) ||
            ts.isInterfaceDeclaration(statement)) &&
          statement.name.text === name,
      );
  const named = (name) => {
    const declaration = find(name);
    if (!declaration) throw new Error(`${entries.join(", ")}: no type ${name}`);
    return checker.getTypeAtLocation(declaration.name);
  };
  return { checker, named, has: (name) => Boolean(find(name)) };
}

const NOTHING =
  ts.TypeFlags.Undefined |
  ts.TypeFlags.Null |
  ts.TypeFlags.Void |
  ts.TypeFlags.Never;
const ANYTHING =
  ts.TypeFlags.Any | ts.TypeFlags.Unknown | ts.TypeFlags.NonPrimitive;

// The members of a union (recursively), without null and undefined.
export function members(type) {
  if (type.isUnion()) return type.types.flatMap(members);
  return type.flags & NOTHING ? [] : [type];
}

function property(checker, type, key) {
  const symbol = checker.getPropertyOfType(type, key);
  return symbol && checker.getTypeOfSymbol(symbol);
}

// The string literal values a member's `key` can take.
export function literals(checker, type, key) {
  const value = property(checker, type, key);
  if (!value) return [];
  return members(value)
    .filter((entry) => entry.isStringLiteral())
    .map((entry) => entry.value);
}

// An object with fields, not a primitive (a branded `string & {...}` is one).
function isObject(type) {
  if (type.isIntersection()) return type.types.every(isObject);
  return Boolean(type.flags & ts.TypeFlags.Object);
}

export class Graph {
  constructor(checker) {
    this.checker = checker;
    this.types = {};
    this.ids = new Map();
  }

  // The type ids a value of `type` may be: `*` for anything, none for a
  // primitive. Arrays are transparent, as `shape.mjs` flattens them; an
  // array already being expanded (`type Json = string | Json[]`) adds nothing.
  refs(type, owner, narrow = new Map(), path = "", arrays = new Set()) {
    const out = new Set();
    for (const entry of members(type)) {
      if (this.checker.isArrayType(entry) || this.checker.isTupleType(entry)) {
        if (arrays.has(entry)) continue;
        const within = new Set(arrays).add(entry);
        for (const item of this.checker.getTypeArguments(entry))
          for (const id of this.refs(item, owner, narrow, path, within))
            out.add(id);
      } else if (entry.flags & ANYTHING) out.add("*");
      else if (isObject(entry)) out.add(this.node(entry, owner, narrow, path));
    }
    return [...out].sort();
  }

  // One object type's fields. A type narrowed below `path` (one union member
  // chosen for a frame kind) is its own node; any other is stored once.
  node(type, owner, narrow, path) {
    const scoped = [...narrow.keys()].some(
      (key) => !path || key.startsWith(`${path}.`),
    );
    if (!scoped && this.ids.has(type)) return this.ids.get(type);
    const symbol = type.aliasSymbol ?? type.symbol;
    const name =
      !scoped && symbol && !symbol.name.startsWith("__") ? symbol.name : owner;
    let id = name;
    for (let suffix = 2; id in this.types; suffix += 1)
      id = `${name}#${suffix}`;
    if (!scoped) this.ids.set(type, id);
    const node = {};
    this.types[id] = node;
    if (this.checker.getIndexInfosOfType(type).length) node["*"] = [];
    for (const field of this.checker.getPropertiesOfType(type)) {
      const declared = this.checker.getTypeOfSymbol(field);
      if (declared.getCallSignatures().length) continue;
      const at = path ? `${path}.${field.name}` : field.name;
      node[field.name] = OPAQUE.has(field.name)
        ? []
        : this.refs(
            narrow.get(at) ?? declared,
            `${id}.${field.name}`,
            narrow,
            at,
          );
    }
    return id;
  }
}

// Splits a frame type into its kinds: for each discriminator path in order
// (`type`, `request.subtype`, `event.delta.type`, ...), one variant per union
// member and literal at that path's container. Returns
// `[{ parts, root, narrow }]`; `parts` joined by "/" is the frame kind.
export function variants(checker, root, paths) {
  let list = [{ parts: [], root, narrow: new Map() }];
  for (const discriminator of paths) {
    const container = discriminator.split(".").slice(0, -1).join(".");
    const key = discriminator.split(".").at(-1);
    list = list.flatMap((variant) => {
      const at = container
        ? typesAt(checker, variant.root, container, variant.narrow)
        : [variant.root];
      if (!at.length) return [variant];
      return at.flatMap(members).flatMap((member) => {
        const narrow = new Map(variant.narrow);
        const next = container ? variant.root : member;
        if (container) narrow.set(container, member);
        const values = literals(checker, member, key);
        if (!values.length)
          return [{ parts: variant.parts, root: next, narrow }];
        return values.map((value) => ({
          parts: [...variant.parts, value],
          root: next,
          narrow,
        }));
      });
    });
  }
  return list;
}

// The types a path can hold, through every union member on the way.
function typesAt(checker, root, path, narrow) {
  let types = [root];
  let walked = "";
  for (const key of path.split(".")) {
    walked = walked ? `${walked}.${key}` : key;
    const forced = narrow.get(walked);
    types = forced
      ? [forced]
      : types
          .flatMap(members)
          .flatMap((entry) => property(checker, entry, key) ?? []);
  }
  return types;
}
