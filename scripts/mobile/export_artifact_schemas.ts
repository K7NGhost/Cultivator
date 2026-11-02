import { artifactModelDefinitions } from "../../src/features/artifacts/model/artifactModels";

// The Python worker and native receiver consume this checked-in snapshot.
// A catalog test verifies that it stays in sync with the TypeScript models.
await Bun.write(new URL("../../src-tauri/src/scripting/worker/artifact_model_schemas.json", import.meta.url),
  JSON.stringify({ version: 1, models: artifactModelDefinitions }, null, 2) + "\n");
