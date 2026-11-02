import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import ts from "typescript";
import { artifactCategoryDefinitions } from "../src/features/artifacts/model/artifactCategoryDefinitions";
import { artifactModelDefinitions, artifactModelsByKind, artifactModelsByCategory } from "../src/features/artifacts/model/artifactModels";
import { getArtifactNavigationCategory } from "../src/features/artifacts/model/artifactNavigationCategories";
import { buildCultivatorApiReferenceForLlms } from "../src/features/artifacts/utils/llmApiReference";
import type { StoredArtifactRecord } from "../src/features/artifacts/model/types";

describe("standard artifact models", () => {
  test("the LEAPP worker's bundled model snapshot matches the canonical schemas", () => {
    const snapshot = JSON.parse(readFileSync("src-tauri/src/scripting/worker/artifact_model_schemas.json", "utf8"));
    expect(snapshot.version).toBe(1);
    expect(snapshot.models).toEqual(artifactModelDefinitions);
  });
  test("every main category owns documented models and routes each kind exactly once", () => {
    expect(artifactCategoryDefinitions).toHaveLength(13);
    expect(artifactModelDefinitions).toHaveLength(45);
    const kinds = artifactCategoryDefinitions.flatMap((category) => [...category.modelKinds]);
    expect(new Set(kinds).size).toBe(kinds.length);
    expect(new Set(artifactModelDefinitions.map((model) => model.kind)).size).toBe(45);
    for (const category of artifactCategoryDefinitions) {
      expect(category.modelKinds).toContain(category.primaryKind);
      expect(artifactModelsByCategory.get(category.id)?.length).toBe(category.modelKinds.length);
      for (const kind of category.modelKinds) {
        const model = artifactModelsByKind.get(kind)!;
        expect(model.navigationCategory).toBe(category.id);
        expect(model.schemaVersion).toBe(1);
        const artifact = { resultKind: kind, payload: { kind, category: model.category } } as StoredArtifactRecord;
        expect(getArtifactNavigationCategory(artifact)).toBe(category.label);
      }
    }
  });

  test("field descriptors cover the TypeScript payloads and identify required fields", () => {
    const file = "src/features/artifacts/model/types.ts";
    const program = ts.createProgram([file], { strict: true, skipLibCheck: true, target: ts.ScriptTarget.ES2020 });
    const checker = program.getTypeChecker();
    const source = program.getSourceFile(file)!;
    const alias = source.statements.find((statement): statement is ts.TypeAliasDeclaration => ts.isTypeAliasDeclaration(statement) && statement.name.text === "Artifact")!;
    const union = checker.getTypeAtLocation(alias) as ts.UnionType;
    const shared = new Set(["kind", "category", "icon", "group", "description", "source", "timestamps", "tags", "confidence", "severity", "raw", "deduplication", "nodeId", "relatedIds"]);
    for (const payload of union.types) {
      const kind = checker.getTypeOfSymbolAtLocation(payload.getProperty("kind")!, alias) as ts.StringLiteralType;
      const model = artifactModelsByKind.get(kind.value as Parameters<typeof artifactModelsByKind.get>[0])!;
      expect(model).toBeDefined();
      expect(new Set(model.fields.map((field) => field.key)).size).toBe(model.fields.length);
      for (const property of payload.getProperties()) {
        if (shared.has(property.name)) continue;
        const field = model.fields.find((field) => field.key === property.name);
        expect(field, `${kind.value}.${property.name}`).toBeDefined();
        expect(!!field?.required, `${kind.value}.${property.name} required`).toBe(!(property.flags & ts.SymbolFlags.Optional));
      }
    }
  });

  test("the Python worker's default categories match every model", () => {
    const worker = readFileSync("src-tauri/src/scripting/worker/plugin.py", "utf8");
    const block = worker.match(/ARTIFACT_CATEGORIES = \{([\s\S]*?)\n\}/)![1];
    const categories = new Map([...block.matchAll(/"([^"]+)": "([^"]+)"/g)].map((match) => [match[1], match[2]]));
    for (const model of artifactModelDefinitions) expect(categories.get(model.kind), model.kind).toBe(model.category);
  });

  test("the generated plugin reference documents the full category catalog", () => {
    const reference = buildCultivatorApiReferenceForLlms();
    for (const category of artifactCategoryDefinitions) expect(reference).toContain(category.id);
    for (const model of artifactModelDefinitions) expect(reference).toContain(`kind: ${model.kind}`);
    expect(reference).toContain("cultivator_api.social_profile(label, **fields)");
  });
});
