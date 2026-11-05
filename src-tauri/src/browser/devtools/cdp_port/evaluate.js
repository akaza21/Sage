export default function serializeDeveloperResult(limit, entries, resume) {
  // Fixed serializer; CDP classifies every object before it is traversed.
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
    const ancestors = new Set();
    const approved = new Set();
    const root = resume ? this.root : this;
    approved.add(root);
    approved.add(Object.prototype);
    // Previous states are immutable, serializer-owned records. Reuse their
    // references without resending a growing argument list across CDP.
    for (let state = resume ? this : null; state; state = state.previous) {
      for (let i = 1; i < state.length; i++) approved.add(state[i]);
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
      if (value !== null && typeof value === "object") {
        if (!approved.has(value)) return defer(value);
        if (!isArray(value)) {
          // for-in and native brand reads can consult prototypes. Verify
          // the current chain synchronously before either operation; an
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
      if (isArray(value)) {
        append("[");
        const length = descriptor(value, "length").value;
        for (let i = 0; i < length; i++) {
          visit();
          if (i) append(",");
          const property = descriptor(value, "" + i);
          encode(
            !property
              ? null
              : hasOwn(property, "value")
                ? property.value
                : "[getter]",
            depth + 1,
          );
        }
        append("]");
      } else {
        append("{");
        let comma = "";
        for (const key in value) {
          visit();
          const property = descriptor(value, key);
          if (!property || !property.enumerable) continue;
          const item = hasOwn(property, "value") ? property.value : "[getter]";
          if (omitted(item)) continue;
          append(comma);
          append(quote(key));
          append(":");
          encode(item, depth + 1);
          comma = ",";
        }
        append("}");
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
      pending.previous = resume ? this : null;
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
