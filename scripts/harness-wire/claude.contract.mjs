// Claude Code's wire contract, from the official Agent SDK's declarations.
// The SDK publishes no JSON Schema; `sdk.d.ts` declares everything the CLI
// writes (`StdoutMessage`) and the control protocol. Each SDK release names
// the CLI it bundles in `claudeCodeVersion`.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

import {
  Graph,
  declarations,
  installed,
  literals,
  members,
  variants,
} from "./contract-ts.mjs";

const SDK = "@anthropic-ai/claude-agent-sdk";

function npm(args) {
  return execFileSync("npm", args, {
    encoding: "utf8",
    shell: process.platform === "win32",
  });
}

// The SDK release that bundles Claude Code `cli` (x.y.z).
function sdkFor(cli) {
  const releases = [].concat(
    JSON.parse(
      npm(["view", `${SDK}@*`, "version", "claudeCodeVersion", "--json"]),
    ),
  );
  const found = releases
    .filter((entry) => entry.claudeCodeVersion === cli)
    .at(-1);
  if (!found) throw new Error(`no ${SDK} release bundles Claude Code ${cli}`);
  return found.version;
}

function add(map, kind, ids) {
  map[kind] = [...new Set([...(map[kind] ?? []), ...ids])].sort();
}

// A frame type split into kinds by its discriminators.
function split(api, graph, map, root, discriminators) {
  for (const variant of variants(api.checker, root, discriminators)) {
    const kind = variant.parts.join("/");
    add(map, kind, graph.refs(variant.root, kind, variant.narrow));
  }
}

// `control_response/<success|error>/<answered subtype>`. A success payload is
// typed where the SDK declares `SDKControl<X>Response` beside the
// `SDKControl<X>Request` it answers, and open otherwise, as the SDK types it.
function responses(api, graph, map) {
  const envelope = api.named("SDKControlResponse");
  for (const request of members(api.named("SDKControlRequestInner"))) {
    const name = request.aliasSymbol?.name ?? "";
    const reply = name.replace(/Request$/, "Response");
    for (const subtype of literals(api.checker, request, "subtype")) {
      for (const variant of variants(api.checker, envelope, [
        "type",
        "response.subtype",
      ])) {
        const narrow = new Map(variant.narrow);
        if (variant.parts[1] === "success" && reply !== name && api.has(reply))
          narrow.set("response.response", api.named(reply));
        const kind = [...variant.parts, subtype].join("/");
        add(map, kind, graph.refs(variant.root, kind, narrow));
      }
    }
  }
}

export async function contract(version, work) {
  const cli = version.match(/\d+\.\d+\.\d+/)[0];
  const sdk = sdkFor(cli);
  const dir = installed(`${SDK}@${sdk}`, work);
  const bundled = JSON.parse(
    fs.readFileSync(path.join(dir, "manifest.json"), "utf8"),
  ).version;
  if (bundled !== cli)
    throw new Error(`${SDK}@${sdk} bundles ${bundled}, not ${cli}`);
  const api = declarations(path.join(dir, "sdk.d.ts"));
  const graph = new Graph(api.checker);
  const inbound = {};
  const outbound = {};
  // What the CLI writes, one `StdoutMessage` per line.
  for (const member of members(api.named("StdoutMessage"))) {
    if (literals(api.checker, member, "type").includes("control_response"))
      continue;
    split(api, graph, inbound, member, [
      "type",
      "subtype",
      "request.subtype",
      "event.type",
      "event.delta.type",
      "event.content_block.type",
    ]);
  }
  responses(api, graph, inbound);
  // What a client writes on stdin: user messages and the control protocol.
  for (const root of [
    "SDKUserMessage",
    "SDKControlRequest",
    "SDKControlCancelRequest",
    "SDKKeepAliveMessage",
  ])
    split(api, graph, outbound, api.named(root), [
      "type",
      "subtype",
      "request.subtype",
    ]);
  responses(api, graph, outbound);
  return {
    version: cli,
    source: `${SDK}@${sdk} sdk.d.ts`,
    in: inbound,
    out: outbound,
    types: graph.types,
  };
}
