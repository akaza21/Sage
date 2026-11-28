// Codex's wire contract, from the JSON Schema the Codex binary generates for
// its own app-server protocol (`codex app-server generate-json-schema`).
// `--experimental` is included because Sage initializes with
// `experimentalApi: true`. A request's `<X>Params` is answered by the
// bundle's `<X>Response`, the generator's pairing; a request without one
// leaves its result open.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

import { OPAQUE } from "./shape.mjs";

const BUNDLE = "codex_app_server_protocol.schemas.json";

class Graph {
  constructor(schema) {
    this.schema = schema;
    this.types = {};
    this.named = {};
  }

  resolve(ref) {
    return ref
      .replace(/^#\//, "")
      .split("/")
      .reduce((node, key) => node?.[key], this.schema);
  }

  // The type ids a value of `schema` may be: `*` for anything, none for a
  // primitive. Arrays are transparent, as `shape.mjs` flattens them.
  refs(schema, owner) {
    if (schema === false) return [];
    if (schema === true || !schema) return ["*"];
    const out = new Set();
    const add = (ids) => ids.forEach((id) => out.add(id));
    const unions = ["anyOf", "oneOf", "allOf"].flatMap(
      (key) => schema[key] ?? [],
    );
    const types = [].concat(schema.type ?? []);
    if (schema.$ref) add(this.definition(schema.$ref));
    unions.forEach((entry, index) =>
      add(this.refs(entry, `${owner}#${index + 1}`)),
    );
    if (types.includes("array") || schema.items)
      add(this.refs(schema.items ?? true, owner));
    if (schema.properties || schema.additionalProperties) {
      let id = owner;
      for (let suffix = 2; id in this.types; suffix += 1)
        id = `${owner}#${suffix}`;
      out.add(this.node(schema, id));
    } else if (
      types.includes("object") ||
      (!types.length &&
        !schema.$ref &&
        !unions.length &&
        !("enum" in schema) &&
        !("const" in schema))
    )
      out.add(
        schema.additionalProperties === false ? this.node(schema, owner) : "*",
      );
    return [...out].sort();
  }

  // A named definition, once; its id is its path under `definitions`.
  definition(ref) {
    const id = ref.replace(/^#\/definitions\//, "");
    if (id in this.named) return this.named[id];
    const target = this.resolve(ref);
    if (target?.properties || target?.additionalProperties) {
      // Shared fields beside a union of variants (`ThreadItem`): the
      // shared node, then each variant.
      this.named[id] = [id];
      this.node(target, id);
      const variants = ["anyOf", "oneOf", "allOf"]
        .flatMap((key) => target[key] ?? [])
        .flatMap((entry, index) => this.refs(entry, `${id}#${index + 1}`));
      this.named[id] = [...new Set([id, ...variants])].sort();
    } else {
      this.named[id] = ["*"];
      this.named[id] = this.refs(target, id);
    }
    return this.named[id];
  }

  node(schema, id) {
    const node = {};
    this.types[id] = node;
    if (schema.additionalProperties) node["*"] = [];
    for (const [field, value] of Object.entries(schema.properties ?? {}))
      node[field] = OPAQUE.has(field) ? [] : this.refs(value, `${id}.${field}`);
    return id;
  }
}

function methodOf(entry) {
  return entry.properties?.method?.enum?.[0];
}

export async function contract(version, work) {
  const wanted = version.match(/\d+\.\d+\.\d+/)[0];
  const installed = execFileSync("codex", ["--version"], {
    encoding: "utf8",
    shell: process.platform === "win32",
  });
  const command = installed.includes(wanted)
    ? ["codex"]
    : ["npx", "-y", `@openai/codex@${wanted}`];
  execFileSync(
    command[0],
    [
      ...command.slice(1),
      "app-server",
      "generate-json-schema",
      "--experimental",
      "--out",
      work,
    ],
    {
      shell: process.platform === "win32",
      stdio: "ignore",
    },
  );
  const schema = JSON.parse(fs.readFileSync(path.join(work, BUNDLE), "utf8"));
  const graph = new Graph(schema);
  const defs = schema.definitions;
  const inbound = {};
  const outbound = {};
  const envelope = (map, kind, ids) => {
    map[kind] = [...new Set([...(map[kind] ?? []), ...ids])].sort();
  };
  // `notify:<method>[:<item type>]` and `request:<method>`, as `shape.mjs`
  // names a JSON-RPC frame.
  const messages = (map, root, prefix) => {
    // Envelope fields every message of `root` carries (`emittedAtMs` on a
    // notification), declared beside its `oneOf`.
    const common = defs[root].properties
      ? [graph.node({ properties: defs[root].properties }, root)]
      : [];
    for (const entry of defs[root].oneOf) {
      const method = methodOf(entry);
      const id = graph.node(entry, `${root}:${method}`);
      const items = itemVariants(graph, entry);
      if (!items.length) envelope(map, `${prefix}:${method}`, [id, ...common]);
      // A kind per item type, its `params.item` narrowed to that variant
      // beside the item's shared fields.
      for (const { type, variant, shared } of items) {
        const kind = `${prefix}:${method}:${type}`;
        const params = graph.types[graph.types[id].params[0]];
        graph.types[`${kind}.params`] = {
          ...params,
          item: [
            ...shared,
            ...graph.refs(variant, `${kind}.params.item`),
          ].sort(),
        };
        graph.types[kind] = { ...graph.types[id], params: [`${kind}.params`] };
        envelope(map, kind, [kind, ...common]);
      }
    }
  };
  // `result:<method>` and `error:<method>` for each request the other side
  // sends.
  const replies = (map, root) => {
    for (const entry of defs[root].oneOf) {
      const method = methodOf(entry);
      const params = entry.properties.params?.$ref?.split("/").at(-1) ?? "";
      const answer = params.replace(/Params$/, "Response");
      const ref = Object.entries({ v2: defs.v2 ?? {}, "": defs })
        .map(([scope, list]) =>
          answer && answer !== params && list[answer]
            ? `#/definitions/${scope ? `${scope}/` : ""}${answer}`
            : "",
        )
        .find(Boolean);
      graph.types[`result:${method}`] = {
        id: [],
        result: ref ? graph.refs({ $ref: ref }, answer) : ["*"],
      };
      envelope(map, `result:${method}`, [`result:${method}`]);
      envelope(
        map,
        `error:${method}`,
        graph.refs({ $ref: "#/definitions/JSONRPCError" }, "JSONRPCError"),
      );
    }
  };
  messages(inbound, "ServerNotification", "notify");
  messages(inbound, "ServerRequest", "request");
  replies(inbound, "ClientRequest");
  messages(outbound, "ClientRequest", "request");
  messages(outbound, "ClientNotification", "notify");
  replies(outbound, "ServerRequest");
  return {
    version: wanted,
    source: `codex app-server generate-json-schema --experimental (${installed.trim()})`,
    in: inbound,
    out: outbound,
    types: graph.types,
  };
}

// Each `params.item.type` a message's item can take, with that variant's
// schema and the node of the fields every variant shares.
function itemVariants(graph, entry) {
  const params = entry.properties?.params;
  const resolved = params?.$ref ? graph.resolve(params.$ref) : params;
  const item = resolved?.properties?.item;
  if (!item) return [];
  const union = item.$ref ? graph.resolve(item.$ref) : item;
  const shared =
    item.$ref && union.properties
      ? [item.$ref.replace(/^#\/definitions\//, "")]
      : [];
  return (union.oneOf ?? union.anyOf ?? []).flatMap((variant) => {
    const schema = variant.$ref ? graph.resolve(variant.$ref) : variant;
    return (schema.properties?.type?.enum ?? []).map((type) => ({
      type,
      variant,
      shared,
    }));
  });
}
