// Per-feature lazy chunk budgets (`bundle.lazyFeatureChunks`).
//
// Each entry names a dynamic-import source entry, its own gzip cap and a
// short reason. The verifier counts only the files that entry owns
// exclusively (its own chunk plus statically imported chunks nothing else
// reaches), subtracts them from the core total and checks them against the
// entry's cap. A feature that is not a reachable dynamic entry, that is
// statically (eagerly) imported, that lands in the initial closure, or whose
// chunk is shared with another manifest entry fails closed.
//
// A feature's CSS (every stylesheet it loads that only budgeted features
// reach, so a stylesheet two features share is budgeted by both) needs its own
// `maxCssGzipBytes` cap, must stay out of the initial CSS, and must not be
// carried by any record outside the budgeted features.

// Finite catalog input capacity, independent of the byte budgets below.
// Agent and Editor features share this catalog, including demand-only tools.
const MAX_LAZY_FEATURES = 96;
const MAX_REASON_LENGTH = 200;

// Editor packs (EDIT-0005 D1): on-demand language grammars and editing
// features, named by a reason that starts with "Editor pack:". Each cap is at
// most 40 KB gzip and all of them together at most 350 KB, so a pack budget
// can never grow past what the owner approved.
const EDITOR_PACK_PREFIX = "Editor pack:";
const MAX_EDITOR_PACK_GZIP_BYTES = 40 * 1024;
const MAX_EDITOR_PACKS_GZIP_BYTES = 350 * 1000;

function fail(message) {
  throw new Error(`web-bundle: ${message}`);
}

/** Validates the budget list; returns [] when the key is absent. */
export function parseLazyFeatureBudgets(value, reservedKeys = []) {
  if (value === undefined) return [];
  if (!Array.isArray(value) || value.length > MAX_LAZY_FEATURES)
    fail("lazy feature chunk budgets are missing or invalid");
  const seen = new Set(reservedKeys);
  for (const feature of value) {
    if (
      !feature ||
      typeof feature !== "object" ||
      ![
        "entry,maxGzipBytes,reason",
        "entry,maxCssGzipBytes,maxGzipBytes,reason",
      ].includes(Object.keys(feature).sort().join()) ||
      (feature.maxCssGzipBytes !== undefined &&
        (!Number.isInteger(feature.maxCssGzipBytes) ||
          feature.maxCssGzipBytes < 1)) ||
      typeof feature.entry !== "string" ||
      !/^src\/[\w./-]+\.(?:ts|tsx)$/u.test(feature.entry) ||
      feature.entry.includes("..") ||
      !Number.isInteger(feature.maxGzipBytes) ||
      feature.maxGzipBytes < 1 ||
      typeof feature.reason !== "string" ||
      feature.reason.trim().length === 0 ||
      feature.reason.length > MAX_REASON_LENGTH ||
      seen.has(feature.entry)
    )
      fail("lazy feature chunk budgets are missing or invalid");
    seen.add(feature.entry);
  }
  const packs = value.filter((feature) =>
    feature.reason.startsWith(EDITOR_PACK_PREFIX),
  );
  if (
    packs.some((pack) => pack.maxGzipBytes > MAX_EDITOR_PACK_GZIP_BYTES) ||
    packs.reduce((sum, pack) => sum + pack.maxGzipBytes, 0) >
      MAX_EDITOR_PACKS_GZIP_BYTES
  )
    fail("editor pack budgets exceed the approved 40 KB each, 350 KB in all");
  return value;
}

function staticClosure(manifest, key) {
  const keys = new Set();
  const pending = [key];
  while (pending.length > 0) {
    const next = pending.pop();
    if (keys.has(next) || next === "index.html") continue;
    const record = manifest[next];
    if (!record) fail(`manifest import is missing: ${next}`);
    keys.add(next);
    pending.push(...(record.imports ?? []));
  }
  return keys;
}

function filesReachableWithout(manifest, startKey, excludedKeys) {
  const keys = new Set();
  const pending = [startKey];
  while (pending.length > 0) {
    const next = pending.pop();
    if (keys.has(next) || excludedKeys.has(next)) continue;
    const record = manifest[next];
    if (!record) fail(`manifest import is missing: ${next}`);
    keys.add(next);
    pending.push(...(record.imports ?? []), ...(record.dynamicImports ?? []));
  }
  return new Set([...keys].map((key) => manifest[key].file));
}

/**
 * Measures each feature's exclusive files and enforces its rules and cap.
 * Returns { [entry]: { gzipBytes, files } } in budget order.
 */
export function measureLazyFeatures({
  manifest,
  entryKey,
  reachable,
  initial,
  isInitialCss = () => false,
  features,
  gzipFile,
}) {
  const result = {};
  // Files reachable without passing through any budgeted feature belong to
  // the initial closure or to unbudgeted chunks. There is no total CSS cap,
  // so every other stylesheet a feature loads counts against that feature,
  // including one it shares with another budgeted feature.
  const unbudgeted = filesReachableWithout(
    manifest,
    entryKey,
    new Set(features.map(({ entry }) => entry)),
  );
  const budgetedKeysFor = (entry) =>
    [...staticClosure(manifest, entry)].filter(
      (key) => !unbudgeted.has(manifest[key].file),
    );
  const budgetedKeys = new Set(
    features
      .filter(({ entry }) => manifest[entry]?.isDynamicEntry)
      .flatMap(({ entry }) => budgetedKeysFor(entry)),
  );
  for (const { entry, maxGzipBytes, maxCssGzipBytes } of features) {
    const record = manifest[entry];
    if (
      !record?.isDynamicEntry ||
      !record.file?.endsWith(".js") ||
      !reachable.has(entry)
    )
      fail(`${entry} must be a reachable dynamic entry`);
    if (initial.has(record.file))
      fail(`${entry} entered the initial static closure`);
    if (
      Object.entries(manifest).some(
        ([key, other]) =>
          key !== entry && (other?.imports ?? []).includes(entry),
      )
    )
      fail(`${entry} is statically imported, so it is not lazy`);
    if (
      Object.entries(manifest).some(
        ([key, other]) => key !== entry && other?.file === record.file,
      )
    )
      fail(`${entry} lazy chunk is shared with another manifest entry`);
    // Files reachable without this entry are excluded, so neither the
    // initial closure nor another feature's chunk can be counted here.
    const elsewhere = filesReachableWithout(
      manifest,
      entryKey,
      new Set([entry]),
    );
    const ownKeys = [...staticClosure(manifest, entry)].filter(
      (key) =>
        manifest[key].file.endsWith(".js") &&
        !elsewhere.has(manifest[key].file),
    );
    const files = [...new Set(ownKeys.map((key) => manifest[key].file))].sort();
    const gzipBytes = files.reduce((sum, file) => sum + gzipFile(file), 0);
    if (gzipBytes > maxGzipBytes)
      fail(`${entry} lazy gzip ${gzipBytes} exceeds ${maxGzipBytes}`);
    const cssFiles = [
      ...new Set(
        budgetedKeysFor(entry).flatMap((key) => manifest[key].css ?? []),
      ),
    ].sort();
    if (cssFiles.some(isInitialCss))
      fail(`${entry} lazy CSS entered the initial CSS`);
    if (
      Object.entries(manifest).some(
        ([key, record]) =>
          !budgetedKeys.has(key) &&
          (record?.css ?? []).some((css) => cssFiles.includes(css)),
      )
    )
      fail(`${entry} lazy CSS is shared with an unbudgeted manifest entry`);
    if (cssFiles.length > 0 && maxCssGzipBytes === undefined)
      fail(`${entry} must budget its lazy CSS (maxCssGzipBytes)`);
    const cssGzipBytes = cssFiles.reduce(
      (sum, file) => sum + gzipFile(file),
      0,
    );
    if (maxCssGzipBytes !== undefined && cssGzipBytes > maxCssGzipBytes)
      fail(`${entry} lazy CSS gzip ${cssGzipBytes} exceeds ${maxCssGzipBytes}`);
    result[entry] = { gzipBytes, files, cssGzipBytes, cssFiles };
  }
  return result;
}
