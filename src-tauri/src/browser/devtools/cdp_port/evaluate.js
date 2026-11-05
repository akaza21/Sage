export default function serializeDeveloperResult(
  receiver,
  limit,
  entries,
  resume,
  candidates,
) {
  // Fixed serializer; CDP rejects proxies before any object is traversed.
  "use strict";
  const capacity = {};
  const unsupported = {};
  try {
    const stringify = JSON.stringify;
    const isArray = Array.isArray;
    const descriptor = Object.getOwnPropertyDescriptor;
    const hasOwn = Object.hasOwn;
    const objectTag = Object.prototype.toString;
    const tagKey = Symbol.toStringTag;
    const typedLength = descriptor(
      Object.getPrototypeOf(Uint8Array.prototype),
      "length",
    ).get;
    const collections = {
      map: {
        size: descriptor(Map.prototype, "size").get,
        iterate: Map.prototype.entries,
        next: Object.getPrototypeOf(new Map().entries()).next,
        pairs: true,
      },
      set: {
        size: descriptor(Set.prototype, "size").get,
        iterate: Set.prototype.values,
        next: Object.getPrototypeOf(new Set().values()).next,
        pairs: false,
      },
    };
    const collectionKind = (value) => {
      // Native admission supplies only a three-bit candidate table, never
      // per-value labels. Ordinary record batches need no brand probes.
      if (!candidates) return 0;
      if (isArray(value)) return 0;
      if (candidates & 4) {
        try {
          typedLength.call(value);
          return 3;
        } catch {
          // A non-typed value (including DataView); try the other brands.
        }
      }
      // A fixed table of intrinsic internal-slot checks derives brands in
      // the page. These never read tags, getters, or conversion overrides,
      // and work for cross-realm values and replaced prototypes as well.
      const table = [collections.map, collections.set];
      for (let i = 0; i < table.length; i++) {
        if (!(candidates & (1 << i))) continue;
        try {
          table[i].size.call(value);
          return i + 1;
        } catch {
          // A native brand mismatch; no page code executes.
        }
      }
      return 0;
    };
    const sizedCollection = (value, kind) => {
      if (isArray(value))
        return { length: descriptor(value, "length").value, indexed: true };
      if (kind === 3)
        return { length: typedLength.call(value), indexed: true, typed: true };
      const collection =
        kind === 1 ? collections.map : kind === 2 ? collections.set : null;
      return collection
        ? {
            length: collection.size.call(value),
            iterate: collection.iterate,
            next: collection.next,
            pairs: collection.pairs,
          }
        : null;
    };
    const ancestors = new Set();
    const approved = new Map();
    const root = resume ? receiver.root : receiver;
    approved.set(Object.prototype, 0);
    approved.set(root, collectionKind(root));
    // Previous states are immutable, serializer-owned records. Reuse their
    // references locally after native proxy admission. No per-value type
    // labels cross CDP, even for deep, wide graphs.
    let currentKinds = "";
    let knownKinds = null;
    for (let state = resume ? receiver : null; state; state = state.previous) {
      for (let i = 1; i < state.length; i++) {
        const kind =
          knownKinds === null ? collectionKind(state[i]) : +knownKinds[i - 1];
        if (knownKinds === null) currentKinds += kind;
        approved.set(state[i], kind);
      }
      knownKinds = state.previousKinds;
    }
    const pending = Object.create(null);
    const discovered = new Set();
    let pendingLength = 1;
    const defer = (value) => {
      if (!discovered.has(value)) {
        if (pendingLength > 4096) throw capacity;
        discovered.add(value);
        pending[pendingLength++] = value;
      }
      append("null");
    };
    const unboxers = [
      Number.prototype.valueOf,
      Boolean.prototype.valueOf,
      String.prototype.valueOf,
      BigInt.prototype.valueOf,
    ];
    const unbox = (value, customTag) => {
      if (!customTag) {
        // The verified chain has no toStringTag property, so this native
        // brand read cannot invoke an accessor. Avoid exception-based brand
        // probes for ordinary records, the common path through the encoder.
        switch (objectTag.call(value)) {
          case "[object Number]":
            return unboxers[0].call(value);
          case "[object Boolean]":
            return unboxers[1].call(value);
          case "[object String]":
            return unboxers[2].call(value);
          default:
            return value;
        }
      }
      for (let i = 0; i < unboxers.length; i++) {
        try {
          return unboxers[i].call(value);
        } catch {
          // Native internal-slot checks never consult conversion overrides.
        }
      }
      return value;
    };
    let visited = 0;
    let prototypeVisits = 0;
    let text = "";
    const append = (part) => {
      if (text.length + part.length > limit) throw capacity;
      text += part;
    };
    const quote = (value) => {
      if (value.length > limit) throw capacity;
      return stringify(value);
    };
    const visit = () => {
      if (visited >= entries) throw capacity;
      visited++;
    };
    const omitted = (value) =>
      typeof value === "undefined" ||
      typeof value === "function" ||
      typeof value === "symbol";
    const encode = (value, depth) => {
      if (depth > 64) throw capacity;
      let collection = null;
      if (value !== null && typeof value === "object") {
        if (!approved.has(value)) return defer(value);
        collection = sizedCollection(value, approved.get(value));
        // O(1) native sizes reject huge collections before enumerating
        // their contents. Admitted contents are traversed lazily.
        if (collection && collection.length > entries - visited) throw capacity;
        if (!collection) {
          // Native brand reads can consult prototypes. Verify
          // the current chain synchronously before unboxing; an
          // unknown prototype is never inspected by JavaScript.
          let prototype = value;
          let customTag = false;
          while (prototype !== null) {
            if (++prototypeVisits > 4096) throw capacity;
            if (!approved.has(prototype)) return defer(prototype);
            if (descriptor(prototype, tagKey)) customTag = true;
            prototype = Object.getPrototypeOf(prototype);
          }
          value = unbox(value, customTag);
        }
      }
      if (value === null || typeof value !== "object") {
        if (typeof value === "bigint") throw unsupported;
        append(
          typeof value === "string"
            ? quote(value)
            : omitted(value)
              ? "null"
              : stringify(value),
        );
        return;
      }
      if (ancestors.has(value)) throw unsupported;
      ancestors.add(value);
      if (collection) {
        append(collection.typed ? "{" : "[");
        const iterator = collection.indexed
          ? null
          : collection.iterate.call(value);
        for (let i = 0; i < collection.length; i++) {
          visit();
          if (i) append(",");
          if (collection.indexed) {
            if (collection.typed) append(quote("" + i) + ":");
            const property = descriptor(value, "" + i);
            encode(
              !property
                ? null
                : hasOwn(property, "value")
                  ? property.value
                  : "[getter]",
              depth + 1,
            );
          } else {
            const item = collection.next.call(iterator).value;
            if (collection.pairs) {
              append("[");
              // The entry pair adds its own JSON array nesting level.
              encode(item[0], depth + 2);
              append(",");
              encode(item[1], depth + 2);
              append("]");
            } else encode(item, depth + 1);
          }
        }
        append(collection.typed ? "}" : "]");
      } else {
        // Neither JS enumeration nor V8's inspector preview is bounded:
        // DebugPropertyIterator collects all keys before its preview
        // accumulator can stop. This marker replaces even small records,
        // including collection members. Read selected paths via evaluate.
        append(
          '{"kind":"object","preview":false,"reason":"not-inspectable-within-budget"}',
        );
      }
      ancestors.delete(value);
    };
    append('{"result":{"type":');
    append(quote(typeof root));
    if (isArray(root)) append(',"subtype":"array"');
    if (!omitted(root)) {
      append(',"value":');
      encode(root, 0);
    }
    append("}}");
    if (pendingLength > 1) {
      pending[0] = visited;
      pending.length = pendingLength;
      pending.root = root;
      pending.previous = resume ? receiver : null;
      // At most one byte per admitted reference, kept only in this call's
      // released scratch group. Earlier passes never repeat brand probes.
      pending.previousKinds = currentKinds;
      return pending;
    }
    let bytes = 0;
    for (let i = 0; i < text.length; i++) {
      const c = text[i];
      if (c <= "\u007f") bytes++;
      else if (c <= "\u07ff") bytes += 2;
      else if (
        c >= "\ud800" &&
        c <= "\udbff" &&
        text[i + 1] >= "\udc00" &&
        text[i + 1] <= "\udfff"
      ) {
        bytes += 4;
        i++;
      } else if (c >= "\ud800" && c <= "\udfff") bytes += 6;
      else bytes += 3;
      if (bytes > limit) throw capacity;
    }
    return text;
  } catch (error) {
    return error === capacity
      ? "capacity"
      : '{"result":{"type":"undefined"},"exceptionDetails":{"text":"Result could not be serialized"}}';
  }
}
