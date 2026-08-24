import { describe, expect, it } from "vitest";

import { assertIde1ProjectExplorerSearchFileJourney } from "./ide1ProjectExplorerSearchFile";

const resource = { rootId: "root-a", relativePath: "src/main.ts" };

function stamp() {
  return {
    projectId: "project-a",
    projectGeneration: 3,
    transitionEpoch: 7,
  };
}

function listing(
  overrides: Record<string, unknown> = {},
  resources = [resource],
) {
  return {
    projectId: "project-a",
    projectGeneration: 3,
    transitionEpoch: 7,
    resources,
    ...overrides,
  };
}

function search(
  overrides: Record<string, unknown> = {},
  matches = [{ resource, lineNumber: 9, columnNumber: 3 }],
) {
  return {
    projectId: "project-a",
    indexGeneration: 3,
    transitionEpoch: 7,
    matches,
    ...overrides,
  };
}

function opened(overrides: Record<string, unknown> = {}) {
  return {
    projectId: "project-a",
    projectGeneration: 3,
    transitionEpoch: 7,
    resource,
    lineNumber: 9,
    columnNumber: 3,
    ...overrides,
  };
}

function bound(overrides: Record<string, unknown> = {}) {
  return {
    stamp: stamp(),
    listing: listing(),
    search: search(),
    opened: opened(),
    ...overrides,
  };
}

function rejectsCode(input: unknown, code: string) {
  try {
    assertIde1ProjectExplorerSearchFileJourney(
      input as Parameters<typeof assertIde1ProjectExplorerSearchFileJourney>[0],
    );
    throw new Error("expected reject");
  } catch (error) {
    expect(error).toBeInstanceOf(Error);
    expect((error as Error & { code?: string }).code).toBe(code);
  }
}

describe("IDE-1 fixture Project Explorer Search File journey", () => {
  it("bound fixture journey returns ok", () => {
    expect(assertIde1ProjectExplorerSearchFileJourney(bound())).toEqual({
      ok: true,
    });
  });

  it("null opened is journey-missing", () => {
    rejectsCode(bound({ opened: null }), "journey-missing");
  });

  it("missing opened key is schema-invalid", () => {
    const { opened: _opened, ...rest } = bound();
    void _opened;
    rejectsCode(rest, "schema-invalid");
  });

  it("listing projectId disagreeing with the stamp is journey-stale", () => {
    rejectsCode(
      bound({ listing: listing({ projectId: "other" }) }),
      "journey-stale",
    );
  });

  it("search projectId disagreeing with the stamp is journey-stale", () => {
    rejectsCode(
      bound({ search: search({ projectId: "other" }) }),
      "journey-stale",
    );
  });

  it("opened projectId disagreeing with the stamp is journey-stale", () => {
    rejectsCode(
      bound({ opened: opened({ projectId: "other" }) }),
      "journey-stale",
    );
  });

  it("listing projectGeneration disagreeing with the stamp is journey-stale", () => {
    rejectsCode(
      bound({ listing: listing({ projectGeneration: 4 }) }),
      "journey-stale",
    );
  });

  it("search indexGeneration disagreeing with the stamp is journey-stale", () => {
    rejectsCode(
      bound({ search: search({ indexGeneration: 4 }) }),
      "journey-stale",
    );
  });

  it("opened projectGeneration disagreeing with the stamp is journey-stale", () => {
    rejectsCode(
      bound({ opened: opened({ projectGeneration: 4 }) }),
      "journey-stale",
    );
  });

  it("listing transitionEpoch disagreeing with the stamp is journey-stale", () => {
    rejectsCode(
      bound({ listing: listing({ transitionEpoch: 8 }) }),
      "journey-stale",
    );
  });

  it("search transitionEpoch disagreeing with the stamp is journey-stale", () => {
    rejectsCode(
      bound({ search: search({ transitionEpoch: 8 }) }),
      "journey-stale",
    );
  });

  it("opened transitionEpoch disagreeing with the stamp is journey-stale", () => {
    rejectsCode(
      bound({ opened: opened({ transitionEpoch: 8 }) }),
      "journey-stale",
    );
  });

  it("opened rootId not in search matches is journey-substituted", () => {
    rejectsCode(
      bound({
        opened: opened({
          resource: { rootId: "root-b", relativePath: "src/main.ts" },
        }),
      }),
      "journey-substituted",
    );
  });

  it("opened relativePath not in search matches is journey-substituted", () => {
    rejectsCode(
      bound({
        opened: opened({
          resource: { rootId: "root-a", relativePath: "src/other.ts" },
        }),
      }),
      "journey-substituted",
    );
  });

  it("opened lineNumber not in search matches is journey-substituted", () => {
    rejectsCode(
      bound({ opened: opened({ lineNumber: 10 }) }),
      "journey-substituted",
    );
  });

  it("opened columnNumber not in search matches is journey-substituted", () => {
    rejectsCode(
      bound({ opened: opened({ columnNumber: 4 }) }),
      "journey-substituted",
    );
  });

  it("matched search rootId absent from the listing is journey-substituted", () => {
    rejectsCode(
      bound({
        listing: listing({}, [
          { rootId: "root-b", relativePath: "src/main.ts" },
        ]),
      }),
      "journey-substituted",
    );
  });

  it("matched search relativePath absent from the listing is journey-substituted", () => {
    rejectsCode(
      bound({
        listing: listing({}, [
          { rootId: "root-a", relativePath: "src/other.ts" },
        ]),
      }),
      "journey-substituted",
    );
  });

  it("schema beats missing and stale", () => {
    const { opened: _opened, ...missing } = bound({
      listing: listing({ projectId: "other" }),
    });
    void _opened;
    rejectsCode(missing, "schema-invalid");
    rejectsCode(
      bound({
        stamp: { ...stamp(), extra: true, projectGeneration: 9 },
        listing: listing({ projectGeneration: 9 }),
        search: search({ indexGeneration: 9 }),
        opened: opened({ projectGeneration: 9 }),
      }),
      "schema-invalid",
    );
  });

  it("null opened beats stale", () => {
    rejectsCode(
      bound({
        listing: listing({ projectId: "other" }),
        opened: null,
      }),
      "journey-missing",
    );
  });

  it("stale beats substitution", () => {
    rejectsCode(
      bound({
        opened: opened({
          projectGeneration: 4,
          resource: { rootId: "root-b", relativePath: "src/main.ts" },
        }),
      }),
      "journey-stale",
    );
  });

  it("extra or missing keys on every journey object are schema-invalid", () => {
    rejectsCode(bound({ extra: true }), "schema-invalid");
    rejectsCode(
      bound({ stamp: { ...stamp(), extra: true } }),
      "schema-invalid",
    );
    rejectsCode(bound({ listing: listing({ extra: true }) }), "schema-invalid");
    rejectsCode(bound({ search: search({ extra: true }) }), "schema-invalid");
    rejectsCode(bound({ opened: opened({ extra: true }) }), "schema-invalid");
    const { projectId: _id, ...stampMissing } = stamp();
    void _id;
    rejectsCode(bound({ stamp: stampMissing }), "schema-invalid");
    rejectsCode(
      bound({
        listing: listing({}, [
          { rootId: "root-a", relativePath: "a.ts", extra: true },
        ]),
      }),
      "schema-invalid",
    );
    rejectsCode(
      bound({
        search: search({}, [
          { resource, lineNumber: 9, columnNumber: 3, extra: true },
        ]),
      }),
      "schema-invalid",
    );
    rejectsCode(
      bound({
        listing: listing({}, [{ rootId: "root-a" }]),
      }),
      "schema-invalid",
    );
  });

  it("wrong container shapes are schema-invalid", () => {
    rejectsCode(bound({ stamp: "nope" }), "schema-invalid");
    rejectsCode(bound({ listing: "nope" }), "schema-invalid");
    rejectsCode(bound({ search: "nope" }), "schema-invalid");
    rejectsCode(bound({ opened: "nope" }), "schema-invalid");
    rejectsCode(
      bound({ listing: listing({ resources: {} }) }),
      "schema-invalid",
    );
    rejectsCode(bound({ search: search({ matches: {} }) }), "schema-invalid");
  });

  it("listing count 0 or 501 is schema-invalid", () => {
    rejectsCode(bound({ listing: listing({}, []) }), "schema-invalid");
    rejectsCode(
      bound({
        listing: listing(
          {},
          Array.from({ length: 501 }, (_, index) => ({
            rootId: "root-a",
            relativePath: `f${index}.ts`,
          })),
        ),
      }),
      "schema-invalid",
    );
  });

  it("search match count 0 or 501 is schema-invalid", () => {
    rejectsCode(bound({ search: search({}, []) }), "schema-invalid");
    rejectsCode(
      bound({
        search: search(
          {},
          Array.from({ length: 501 }, (_, index) => ({
            resource: { rootId: "root-a", relativePath: `f${index}.ts` },
            lineNumber: 1,
            columnNumber: 1,
          })),
        ),
      }),
      "schema-invalid",
    );
  });

  it("duplicate listing resourceKey is schema-invalid", () => {
    rejectsCode(
      bound({ listing: listing({}, [resource, { ...resource }]) }),
      "schema-invalid",
    );
  });

  it("identifier path generation epoch line and column boundaries at every owner field are schema-invalid", () => {
    for (const projectId of ["", "a\0b", `${"a".repeat(513)}`]) {
      rejectsCode(
        bound({ stamp: { ...stamp(), projectId } }),
        "schema-invalid",
      );
      rejectsCode(bound({ listing: listing({ projectId }) }), "schema-invalid");
      rejectsCode(bound({ search: search({ projectId }) }), "schema-invalid");
      rejectsCode(bound({ opened: opened({ projectId }) }), "schema-invalid");
    }
    for (const relativePath of [
      "src\\main.ts",
      "/src/main.ts",
      "src/../x.ts",
    ]) {
      const bad = { rootId: "root-a", relativePath };
      rejectsCode(bound({ listing: listing({}, [bad]) }), "schema-invalid");
      rejectsCode(
        bound({
          search: search({}, [
            { resource: bad, lineNumber: 9, columnNumber: 3 },
          ]),
        }),
        "schema-invalid",
      );
      rejectsCode(
        bound({ opened: opened({ resource: bad }) }),
        "schema-invalid",
      );
    }
    for (const value of [-1, 1.5]) {
      rejectsCode(
        bound({ stamp: { ...stamp(), projectGeneration: value } }),
        "schema-invalid",
      );
      rejectsCode(
        bound({ listing: listing({ projectGeneration: value }) }),
        "schema-invalid",
      );
      rejectsCode(
        bound({ search: search({ indexGeneration: value }) }),
        "schema-invalid",
      );
      rejectsCode(
        bound({ opened: opened({ projectGeneration: value }) }),
        "schema-invalid",
      );
      rejectsCode(
        bound({ stamp: { ...stamp(), transitionEpoch: value } }),
        "schema-invalid",
      );
      rejectsCode(
        bound({ listing: listing({ transitionEpoch: value }) }),
        "schema-invalid",
      );
      rejectsCode(
        bound({ search: search({ transitionEpoch: value }) }),
        "schema-invalid",
      );
      rejectsCode(
        bound({ opened: opened({ transitionEpoch: value }) }),
        "schema-invalid",
      );
    }
    for (const value of [0, 0x1_0000_0000]) {
      rejectsCode(
        bound({ opened: opened({ lineNumber: value }) }),
        "schema-invalid",
      );
      rejectsCode(
        bound({ opened: opened({ columnNumber: value }) }),
        "schema-invalid",
      );
      rejectsCode(
        bound({
          search: search({}, [
            { resource, lineNumber: value, columnNumber: 3 },
          ]),
        }),
        "schema-invalid",
      );
      rejectsCode(
        bound({
          search: search({}, [
            { resource, lineNumber: 9, columnNumber: value },
          ]),
        }),
        "schema-invalid",
      );
    }
  });

  it("rejects throw Error with code not message-only", () => {
    try {
      assertIde1ProjectExplorerSearchFileJourney(bound({ opened: null }));
      throw new Error("expected reject");
    } catch (error) {
      expect(error).toBeInstanceOf(Error);
      expect((error as Error).message).toBe("journey-missing");
      expect((error as Error & { code: string }).code).toBe("journey-missing");
    }
  });

  it("journey codes never become ok", () => {
    expect(() =>
      assertIde1ProjectExplorerSearchFileJourney(bound({ opened: null })),
    ).toThrow();
    rejectsCode(bound({ opened: null }), "journey-missing");
    rejectsCode(
      bound({ listing: listing({ projectId: "other" }) }),
      "journey-stale",
    );
    rejectsCode(
      bound({ opened: opened({ lineNumber: 99 }) }),
      "journey-substituted",
    );
  });
});
