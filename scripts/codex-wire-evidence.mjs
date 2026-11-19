// Regenerate the protocol evidence from real app-server schema directories.
// Usage: node scripts/codex-wire-evidence.mjs <version=schema-directory> ...
import { readFileSync, writeFileSync, readdirSync } from "node:fs";
import { resolve, join } from "node:path";
import { createHash } from "node:crypto";

const inputs = process.argv.slice(2).map((argument) => {
  const separator = argument.indexOf("=");
  if (separator < 1) throw new Error("Expected version=schema-directory");
  return [argument.slice(0, separator), argument.slice(separator + 1)];
});
if (!inputs.length)
  throw new Error("Expected version=schema-directory arguments");
const binding = resolve("src-tauri/src/acp/bindings/codex");
const read = (file) => readFileSync(file, "utf8");
const requestSource = read(join(binding, "request.rs"));
const clientMethods = [...requestSource.matchAll(/=> "([^"]+)"/g)].map(
  (m) => m[1],
);
clientMethods.push(
  "account/read",
  "account/login/start",
  "account/login/cancel",
  "account/logout",
);
const incomingSource = read(join(binding, "incoming/notifications.rs"));
const notifications = [...incomingSource.matchAll(/"([^"\n]+)" =>/g)].map(
  (m) => m[1],
);
const serverMethods = [
  "item/tool/call",
  "currentTime/read",
  "item/tool/requestUserInput",
  "mcpServer/elicitation/request",
  "item/commandExecution/requestApproval",
  "item/fileChange/requestApproval",
  "item/permissions/requestApproval",
];
notifications.push("account/login/completed");
const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");
function clean(value) {
  if (Array.isArray(value)) return value.map(clean);
  if (!value || typeof value !== "object") return value;
  return Object.fromEntries(
    Object.keys(value)
      .sort()
      .filter(
        (key) =>
          ![
            "description",
            "title",
            "$schema",
            "definitions",
            "examples",
          ].includes(key),
      )
      .map((key) => {
        // Schema annotations and wire members can have the same name. Preserve
        // properties.description/title and literal default/enum objects.
        if (key === "properties" || key === "patternProperties") {
          return [
            key,
            Object.fromEntries(
              Object.keys(value[key])
                .sort()
                .map((field) => [field, clean(value[key][field])]),
            ),
          ];
        }
        return [
          key,
          ["default", "enum", "const"].includes(key)
            ? value[key]
            : clean(value[key]),
        ];
      }),
  );
}
const builds = inputs.map(([version, directory]) => {
  const definitions = {},
    wire = {};
  const load = (file) => {
    const schema = JSON.parse(read(join(directory, file)));
    Object.assign(definitions, schema.definitions);
    return schema;
  };
  const responseFiles = ["", "v1", "v2"].flatMap((folder) =>
    readdirSync(join(directory, folder))
      .filter((file) => file.endsWith("Response.json"))
      .map((file) => join(folder, file)),
  );
  for (const file of responseFiles) {
    const schema = load(file);
    definitions[schema.title] = schema;
  }
  for (const [file, methods, prefix] of [
    ["ClientRequest.json", clientMethods, "client"],
    ["ServerNotification.json", notifications, "notification"],
    ["ServerRequest.json", serverMethods, "server"],
  ]) {
    const document = load(file);
    wire[`envelope:${file.replace(".json", "")}`] = clean(
      Object.fromEntries(
        Object.entries(document).filter(([key]) => key !== "oneOf"),
      ),
    );
    for (const variant of document.oneOf) {
      const method = variant.properties.method.enum[0];
      if (!methods.includes(method)) continue;
      wire[`${prefix}:${method}`] = clean(variant);
      if (prefix === "notification") continue;
      const name = variant.properties.params?.$ref?.split("/").at(-1);
      const response =
        {
          "config/mcpServer/reload": "McpServerRefreshResponse",
          "account/logout": "LogoutAccountResponse",
          "account/rateLimits/read": "GetAccountRateLimitsResponse",
        }[method] ?? name?.replace(/Params$/, "Response");
      if (response && definitions[response])
        wire[`${prefix === "client" ? "response" : "reply"}:${method}`] = {
          $ref: `#/definitions/${response}`,
        };
      else throw new Error(`Missing response for ${method}: ${response}`);
    }
  }
  const clientNotifications = load("ClientNotification.json");
  wire["envelope:ClientNotification"] = clean(
    Object.fromEntries(
      Object.entries(clientNotifications).filter(([key]) => key !== "oneOf"),
    ),
  );
  const initialized = clientNotifications.oneOf.find(
    (variant) => variant.properties.method.enum[0] === "initialized",
  );
  if (!initialized) throw new Error("Missing initialized notification");
  wire["clientNotification:initialized"] = clean(initialized);
  for (const name of ["JSONRPCResponse", "JSONRPCError"])
    wire[`envelope:${name}`] = clean(load(`${name}.json`));
  for (const name of ["SandboxPolicy", "AskForApproval", "SandboxMode"])
    wire[`policy:${name}`] = { $ref: `#/definitions/${name}` };
  const used = {};
  function visit(schema) {
    if (!schema || typeof schema !== "object") return;
    if (schema.$ref) {
      const name = schema.$ref.split("/").at(-1);
      if (!(name in used)) {
        if (!definitions[name]) throw new Error(`Missing ${name}`);
        used[name] = clean(definitions[name]);
        if (name === "TurnStartParams") {
          const fields = [
            "threadId",
            "clientUserMessageId",
            "input",
            "approvalPolicy",
            "sandboxPolicy",
            "model",
            "effort",
          ];
          used[name].properties = Object.fromEntries(
            Object.entries(used[name].properties).filter(([field]) =>
              fields.includes(field),
            ),
          );
        }
        if (name === "ThreadResumeParams") {
          const fields = [
            "threadId",
            "model",
            "cwd",
            "approvalPolicy",
            "sandbox",
            "excludeTurns",
            "config",
          ];
          used[name].properties = Object.fromEntries(
            Object.entries(used[name].properties).filter(([field]) =>
              fields.includes(field),
            ),
          );
        }
        // The normalizer reports unknown item kinds as unsupported. It never
        // reads the native function-call-output body, so it is outside Sage's
        // wire subset (including its independently evolving audio encoding).
        if (name === "ThreadItem")
          used[name].oneOf = used[name].oneOf.filter(
            (variant) =>
              variant.properties.type.enum[0] !== "functionCallOutput",
          );
        visit(used[name]);
      }
    }
    for (const child of Object.values(schema)) visit(child);
  }
  visit(wire);
  return {
    version,
    schemaSha256: hash(
      read(join(directory, "codex_app_server_protocol.schemas.json")),
    ),
    wire: clean(wire),
    definitions: clean(used),
  };
});
// Deduplicate identical definitions across versions. Evidence stays lossless for
// every selected wire shape, including defaults and validation constraints.
const schemas = [],
  interned = new Map();
function intern(value) {
  const encoded = JSON.stringify(value);
  if (!interned.has(encoded)) {
    interned.set(encoded, schemas.length);
    schemas.push(value);
  }
  return interned.get(encoded);
}
for (const build of builds) {
  build.wire = Object.fromEntries(
    Object.entries(build.wire).map(([key, value]) => [key, intern(value)]),
  );
  build.definitions = Object.fromEntries(
    Object.entries(build.definitions).map(([key, value]) => [
      key,
      intern(value),
    ]),
  );
}
writeFileSync(
  "src-tauri/testdata/acp/codex/observed-wire-subset.json",
  JSON.stringify(
    {
      contractVersion: "0.167.0",
      exactDefinitions: [
        "ThreadGoalClearedNotification",
        "ThreadNameUpdatedNotification",
        "AccountLoginCompletedNotification",
        "AccountUpdatedNotification",
        "LoginAccountResponse",
        "LogoutAccountResponse",
        "CancelLoginAccountResponse",
      ],
      // Error classification has a bounded fallback for unknown error variants.
      openUnionDefinitions: ["CodexErrorInfo"],
      schemas,
      builds,
    },
    null,
    2,
  ) + "\n",
);
