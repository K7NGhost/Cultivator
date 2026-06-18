import { artifactModelDefinitions } from "@/features/artifacts/artifactModels";

export function buildCultivatorApiReferenceForLlms() {
  const artifactModels = artifactModelDefinitions
    .map((model) => {
      const fields = model.fields
        .map((field) => {
          const required = field.required ? " required" : " optional";

          return `  - ${field.key}: ${field.type},${required}`;
        })
        .join("\n");

      return [
        `### ${model.label}`,
        `kind: ${model.kind}`,
        `category: ${model.category}`,
        `description: ${model.description}`,
        "fields:",
        fields,
      ].join("\n");
    })
    .join("\n\n");

  return `# Cultivator Python Plugin API Reference

Use this reference when writing Python plugins for Cultivator, a Tauri-based forensic analysis application.

## Plugin Shape

Each plugin has a plugin.toml:

\`\`\`toml
id = "example-plugin"
name = "Example Plugin"
description = "Extracts artifacts from logical files."
type = "contacts"
mode = "path_glob"
path_glob = "*/voice/asr/context/phonebook/*.txt"
entry = "plugin.py"
function = "run"
\`\`\`

The Python entrypoint receives a context dictionary:

\`\`\`python
def run(context):
    file_path = context["file"]["path"]
    return None
\`\`\`

## Context

- context["case"]["id"]: str
- context["case"]["database_path"]: str
- context["case"]["folder_path"]: str
- context["case"]["artifacts_path"]: str
- context["datasource"]["id"]: str
- context["datasource"]["name"]: str
- context["datasource"]["paths"]: list[str]
- context["plugin"]["id"]: str
- context["plugin"]["name"]: str
- context["plugin"]["mode"]: "each_file" | "path_glob" | "path_regex"
- context["file"]["path"]: str
- context["file"]["name"]: str
- context["file"]["extension"]: str
- context["file"]["size"]: int

## cultivator_api

\`\`\`python
import cultivator_api

cultivator_api.read_bytes(path: str, max_bytes: int | None = None) -> bytes
cultivator_api.read_text(path: str, max_bytes: int | None = None) -> str
cultivator_api.sha256(path: str) -> str
cultivator_api.log(level: str, message: str) -> None

cultivator_api.search(
    query: str,
    regex: bool = False,
    case_sensitive: bool = False,
    binary_files: bool = False,
    max_matches: int | None = None,
) -> list[SearchMatch]

cultivator_api.search_files(
    root_path: str,
    query: str,
    regex: bool = False,
    case_sensitive: bool = False,
    binary_files: bool = False,
    max_matches: int | None = None,
) -> list[SearchMatch]

cultivator_api.create_artifact(kind: str, label: str, **fields) -> dict
cultivator_api.add_artifact(artifact: dict, file_path: str | None = None) -> None
\`\`\`

Artifact helper functions create dictionaries with kind, category, and label:

\`\`\`python
cultivator_api.account(label, **fields)
cultivator_api.application(label, **fields)
cultivator_api.browser_history(label, **fields)
cultivator_api.call(label, **fields)
cultivator_api.contact(label, **fields)
cultivator_api.credential(label, **fields)
cultivator_api.file_artifact(label, **fields)
cultivator_api.location(label, **fields)
cultivator_api.media(label, **fields)
cultivator_api.message(label, **fields)
cultivator_api.note(label, **fields)
cultivator_api.system_artifact(label, **fields)
cultivator_api.timeline_event(label, **fields)
\`\`\`

Plugins can either return one artifact dictionary, return a list of artifact dictionaries, or call add_artifact() and return None.

## Example

\`\`\`python
import cultivator_api

def run(context):
    artifact = cultivator_api.contact(
        "Ada Lovelace",
        name="Ada Lovelace",
        phones=["+1-555-0100"],
        source={"filePath": context["file"]["path"]},
    )

    cultivator_api.add_artifact(artifact)
    return None
\`\`\`

## Artifact Models

All artifacts share these optional base fields:
- kind: str
- category: str
- label: str
- description: str
- source: {"datasourceId"?: str, "filePath": str, "line"?: int, "column"?: int, "offset"?: int, "length"?: int, "parser"?: str}
- timestamps: list[{"value": str, "label"?: str, "timezone"?: str, "source"?: str}]
- tags: list[str]
- confidence: "low" | "medium" | "high"
- severity: "info" | "low" | "medium" | "high" | "critical"
- raw: any

${artifactModels}
`;
}
