export default function serializeDeveloperResult(limit) {
  // Fixed serializer for Runtime.callFunctionOn; never evaluates agent source.
  "use strict";
  const capacity = {};
  try {
    const stringify = JSON.stringify;
    const isArray = Array.isArray;

    // Charge ordinary values before JSON escaping, then check exact UTF-8
    // bytes. Getters, toJSON and boxed values still run inside the renderer;
    // only admitted text crosses CDP, under its separate envelope budget.
    const encode = (value) => {
      let used = 0;
      const text = stringify(value, function (key, item) {
        const type = typeof item;
        if (type === "undefined" || type === "function" || type === "symbol") {
          if (!isArray(this)) return item;
        }
        if (key && !isArray(this)) used += key.length + 3;
        used +=
          type === "string"
            ? item.length + 2
            : item === null || type !== "object"
              ? type === "number" || type === "boolean"
                ? ("" + item).length
                : 4
              : 2;
        if (used > limit) throw capacity;
        return item;
      });
      if (typeof text !== "string" || text.length > limit) throw capacity;
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
    };

    const result = { type: typeof this };
    if (isArray(this)) result.subtype = "array";
    if (typeof this !== "function") result.value = this;
    return encode({ result });
  } catch (error) {
    return error === capacity
      ? "capacity"
      : '{"result":{"type":"undefined"},"exceptionDetails":{"text":"Result could not be serialized"}}';
  }
}
