import { utf8ByteLength } from "./text";

// ACP v1 only. Configuration is an ephemeral exact-runtime overlay, not a fact.
export interface AcpSessionMode {
  id: string;
  name: string;
  description?: string;
}
export interface AcpSessionModes {
  currentModeId: string;
  availableModes: AcpSessionMode[];
}
export interface AcpConfigChoice {
  value: string;
  name: string;
  description?: string;
}
export interface AcpConfigGroup {
  group: string;
  name: string;
  options: AcpConfigChoice[];
}
interface ConfigBase {
  id: string;
  name: string;
  description?: string;
  category?: string;
}
export type AcpConfigOption = ConfigBase &
  (
    | {
        type: "select";
        currentValue: string;
        options: AcpConfigChoice[] | AcpConfigGroup[];
      }
    | { type: "boolean"; currentValue: boolean }
    | { type: "unsupported"; kind: string }
  );
export type AcpConfigValue =
  { value: string } | { type: "boolean"; value: boolean };
export interface AcpSessionConfig {
  sageAgentSessionId: string;
  runtimeEpoch: string;
  configRevision: string;
  removed?: boolean;
  confirmedModel?: { id: string; name?: string };
  modes: AcpSessionModes | null;
  configOptions: AcpConfigOption[];
}
export interface AcpSessionConfigChangedV1 {
  schemaVersion: 1;
  sageProjectId: string;
  sageChatId: string;
  sageAgentSessionId: string;
  runtimeEpoch: string;
  configRevision: string;
}
const object = (v: unknown): v is Record<string, unknown> =>
  v !== null && typeof v === "object" && !Array.isArray(v);
const text = (v: unknown, max = 1024, required = true): v is string =>
  typeof v === "string" &&
  (!required || v.length > 0) &&
  !v.includes("\0") &&
  utf8ByteLength(v) <= max;
const counter = (v: unknown, positive = false): v is string =>
  typeof v === "string" &&
  /^(0|[1-9][0-9]{0,18})$/.test(v) &&
  BigInt(v) <= 9223372036854775807n &&
  (!positive || v !== "0");
const array = (v: unknown): v is unknown[] =>
  Array.isArray(v) && v.length <= 128;
const unique = (values: string[]) => new Set(values).size === values.length;
function description(
  v: Record<string, unknown>,
): { description?: string } | null {
  return v.description === undefined
    ? {}
    : text(v.description, 16384, false)
      ? { description: v.description }
      : null;
}
function choice(v: unknown): AcpConfigChoice | null {
  if (
    !object(v) ||
    !text(v.value) ||
    !text(v.name, 4096) ||
    "group" in v ||
    "options" in v
  )
    return null;
  const desc = description(v);
  return desc ? { value: v.value, name: v.name, ...desc } : null;
}
function option(v: unknown): AcpConfigOption | null {
  if (
    !object(v) ||
    !text(v.id) ||
    !text(v.name, 4096) ||
    (v.category !== undefined && !text(v.category))
  )
    return null;
  const desc = description(v);
  if (!desc) return null;
  const base = {
    id: v.id,
    name: v.name,
    ...desc,
    ...(v.category === undefined ? {} : { category: v.category as string }),
  };
  if (v.type === "boolean" && typeof v.currentValue === "boolean")
    return { ...base, type: "boolean", currentValue: v.currentValue };
  if (v.type === "unsupported")
    return {
      ...base,
      type: "unsupported",
      kind: text(v.kind, 1024, false) ? v.kind : "",
    };
  if (v.type !== "select")
    return {
      ...base,
      type: "unsupported",
      kind: text(v.type, 1024, false) ? v.type : "",
    };
  if (!text(v.currentValue) || !array(v.options)) return null;
  let options: AcpConfigChoice[] | AcpConfigGroup[];
  let values: string[];
  if (v.options.length > 0 && object(v.options[0]) && "group" in v.options[0]) {
    const groups: AcpConfigGroup[] = [];
    for (const group of v.options) {
      if (
        !object(group) ||
        !text(group.group) ||
        !text(group.name, 4096) ||
        "value" in group ||
        !array(group.options)
      )
        return null;
      const choices = group.options.map(choice);
      if (choices.some((c) => c === null)) return null;
      groups.push({
        group: group.group,
        name: group.name,
        options: choices as AcpConfigChoice[],
      });
    }
    if (!unique(groups.map((g) => g.group))) return null;
    options = groups;
    values = groups.flatMap((g) => g.options.map((c) => c.value));
  } else {
    const choices = v.options.map(choice);
    if (choices.some((c) => c === null)) return null;
    options = choices as AcpConfigChoice[];
    values = options.map((c) => c.value);
  }
  if (!unique(values) || !values.includes(v.currentValue)) return null;
  return { ...base, type: "select", currentValue: v.currentValue, options };
}
function modes(v: unknown): AcpSessionModes | null {
  if (!object(v) || !text(v.currentModeId) || !array(v.availableModes))
    return null;
  const availableModes: AcpSessionMode[] = [];
  for (const mode of v.availableModes) {
    if (!object(mode) || !text(mode.id) || !text(mode.name, 4096)) return null;
    const desc = description(mode);
    if (!desc) return null;
    availableModes.push({ id: mode.id, name: mode.name, ...desc });
  }
  if (
    !unique(availableModes.map((m) => m.id)) ||
    !availableModes.some((m) => m.id === v.currentModeId)
  )
    return null;
  return { currentModeId: v.currentModeId, availableModes };
}
export function decodeSessionConfig(v: unknown): AcpSessionConfig | null {
  if (
    !object(v) ||
    !text(v.sageAgentSessionId) ||
    !counter(v.runtimeEpoch) ||
    !counter(v.configRevision, true) ||
    !array(v.configOptions)
  )
    return null;
  const decodedModes = v.modes === null ? null : modes(v.modes);
  if (v.modes !== null && !decodedModes) return null;
  const removed = v.removed === undefined ? false : v.removed === true;
  if (v.removed !== undefined && !removed) return null;
  const configOptions = v.configOptions.map(option);
  if (configOptions.some((o) => o === null)) return null;
  const options = configOptions as AcpConfigOption[];
  if (!unique(options.map((o) => o.id))) return null;
  const confirmedModel = v.confirmedModel;
  if (
    confirmedModel !== undefined &&
    (!object(confirmedModel) ||
      !text(confirmedModel.id, 512) ||
      (confirmedModel.name !== undefined && !text(confirmedModel.name, 512)))
  )
    return null;
  if (
    removed &&
    (decodedModes !== null ||
      options.length > 0 ||
      confirmedModel !== undefined)
  )
    return null;
  let entries =
    options.length +
    (decodedModes?.availableModes.length ?? 0) +
    Number(confirmedModel !== undefined);
  for (const o of options)
    if (o.type === "select")
      for (const c of o.options)
        entries += "group" in c ? 1 + c.options.length : 1;
  const payload = {
    modes: decodedModes,
    configOptions: options,
    ...(confirmedModel === undefined
      ? {}
      : {
          confirmedModel: {
            id: confirmedModel.id as string,
            ...(confirmedModel.name === undefined
              ? {}
              : { name: confirmedModel.name as string }),
          },
        }),
  };
  if (entries > 128 || utf8ByteLength(JSON.stringify(payload)) > 131072)
    return null;
  return {
    sageAgentSessionId: v.sageAgentSessionId,
    runtimeEpoch: v.runtimeEpoch,
    configRevision: v.configRevision,
    ...(removed ? { removed: true } : {}),
    ...payload,
  };
}
export function decodeSessionConfigNotice(
  v: unknown,
): AcpSessionConfigChangedV1 | null {
  if (
    !object(v) ||
    v.schemaVersion !== 1 ||
    !text(v.sageProjectId) ||
    !text(v.sageChatId) ||
    !text(v.sageAgentSessionId) ||
    !counter(v.runtimeEpoch) ||
    !counter(v.configRevision, true)
  )
    return null;
  return {
    schemaVersion: 1,
    sageProjectId: v.sageProjectId,
    sageChatId: v.sageChatId,
    sageAgentSessionId: v.sageAgentSessionId,
    runtimeEpoch: v.runtimeEpoch,
    configRevision: v.configRevision,
  };
}
