# Rust Built-in Plugin Guide

Rust built-in plugins run inside Cultivator and publish their work through a shared service context. The central plugin runner handles jobs, persistence, progress events, options, cancellation, and datasource updates.

Use a Rust built-in when the plugin needs native performance, direct Rust library access, or tight integration with filesystem processing. Use the Python plugin API for plugins that should be installable without recompiling Cultivator.

## Plugin lifecycle

Every built-in plugin:

1. Supplies a manifest for the plugin UI and path matching.
2. Implements `BuiltinPlugin::run()`.
3. Publishes logs, artifacts, media, derived paths, and progress through `BuiltinPluginContext`.
4. Self-registers with `register_builtin_plugin!`.

The plugin does not create database jobs or write plugin database tables directly.

## Minimal plugin

Create a file under `src-tauri/src/plugins/builtin`, for example `hash_calculator.rs`:

```rust
use super::{
    register_builtin_plugin, BuiltinArtifact, BuiltinPlugin,
    BuiltinPluginContext,
};
use crate::plugins::{
    PythonPluginManifest, PythonPluginMode, PythonPluginTarget,
};
use serde_json::json;

struct HashCalculatorPlugin;

impl HashCalculatorPlugin {
    const fn new() -> Self {
        Self
    }
}

impl BuiltinPlugin for HashCalculatorPlugin {
    fn manifest(&self) -> PythonPluginManifest {
        PythonPluginManifest {
            id: "hash-calculator".to_string(),
            name: "Hash Calculator".to_string(),
            organization_folder: None,
            author: "Your Name".to_string(),
            version: "1.0.0".to_string(),
            description: "Calculates hashes for datasource files.".to_string(),
            plugin_type: "other".to_string(),
            target: PythonPluginTarget::Other,
            mode: PythonPluginMode::PathRegex,
            path_glob: Vec::new(),
            path_regex: Some(".*".to_string()),
            entry: "builtin:hash-calculator".to_string(),
            function: "run".to_string(),
            options: Vec::new(),
        }
    }

    fn run(
        &self,
        context: &mut BuiltinPluginContext<'_>,
    ) -> Result<(), String> {
        let paths = context.paths.clone();
        let total = paths.len() as u64;
        context.replace_artifacts(Vec::new());

        for (index, path) in paths.into_iter().enumerate() {
            if context.is_cancelled() {
                break;
            }

            let hash = calculate_sha256(&path)?;

            context.add_artifact(BuiltinArtifact {
                file_path: path.clone(),
                kind: "file_hash".to_string(),
                label: "SHA-256".to_string(),
                payload: json!({ "algorithm": "SHA-256", "hash": hash }),
            });

            context.progress(index as u64 + 1, total, path);
        }

        context.log("info", "Hash calculation complete.");
        Ok(())
    }
}

register_builtin_plugin!(
    register_hash_calculator,
    HASH_CALCULATOR,
    HashCalculatorPlugin
);
```

Declare the module in `src-tauri/src/plugins/builtin/mod.rs`:

```rust
mod hash_calculator;
```

No change to `plugins.rs`, the database persistence code, or a central plugin array is required.

## Context API

### Datasource paths

```rust
let paths = context.paths.clone();
```

Clone the paths before a loop that mutably calls the context. This avoids borrowing `context.paths` while also reporting results through `context`.

Paths can contain files or directories. A plugin decides whether to process a file directly or recursively walk a directory.

### Progress

```rust
context.progress(completed, total, "Reading browser database");
```

Progress is forwarded to the plugin toast. The plugin supplies the real completed and total values; Cultivator does not estimate them.

Use `total = 0` when the total is genuinely unknown. Do not perform a separate pre-scan solely to calculate a total.

### Cancellation

```rust
if context.is_cancelled() {
    break;
}
```

Check cancellation inside long loops and before expensive operations. Returning `Ok(())` after observing cancellation is acceptable; the runner checks cancellation again before committing results.

### Logs

```rust
context.log("info", "Started processing");
context.log("warn", "A recoverable record was skipped");
context.log("error", "A file could not be parsed");
```

Supported conventional levels are `info`, `warn`, and `error`.

### Artifacts

Publish artifacts incrementally:

```rust
context.add_artifact(BuiltinArtifact {
    file_path: source_path,
    kind: "browser_history".to_string(),
    label: "Browser History".to_string(),
    payload: serde_json::json!({
        "url": url,
        "title": title,
        "visitedAt": visited_at,
    }),
});
```

Artifacts are persisted only after the plugin completes successfully. Call `replace_artifacts(Vec::new())` before scanning when the run should authoritatively replace previous results, including when it finds zero artifacts. Subsequent `add_artifact` calls append to that replacement collection.

To deliberately clear all previous artifacts without adding new ones:

```rust
context.replace_artifacts(Vec::new());
```

### Media gallery entries

Plugins that produce the Media page gallery can replace its datasource collection:

```rust
context.replace_media(MediaGallery {
    photos,
    videos,
    scanned_files,
});
```

Use the shared `MediaGallery`, `MediaItem`, and `MediaType` types from the built-in module. Calling `replace_media` with empty vectors clears the previous media collection for that datasource.

### Derived paths

A plugin that creates a filesystem output can add it to the datasource:

```rust
context.add_derived_path(output_directory.to_string_lossy());
```

After a successful run, Cultivator persists the path and makes it available to later plugins and the tree viewer.

## Output directories

Override `output_directory_name()` when the plugin needs a managed output directory:

```rust
fn output_directory_name(&self) -> Option<&'static str> {
    Some("carved-files")
}
```

The runner then provides `context.output_root` under:

```text
<case>/artifacts/<directory-name>/<datasource-id>
```

Access it safely:

```rust
let output_root = context
    .output_root
    .clone()
    .ok_or_else(|| "Missing plugin output directory".to_string())?;
```

Only call `add_derived_path` when the output should become part of the datasource.

## Options

Declare options in the manifest:

```rust
use crate::plugins::{PluginOptionChoice, PluginOptionDefinition};

options: vec![PluginOptionDefinition {
    id: "hashAlgorithm".to_string(),
    label: "Hash algorithm".to_string(),
    description: "Algorithm used for file hashing.".to_string(),
    option_type: "select".to_string(),
    default_value: "sha256".to_string(),
    choices: vec![
        PluginOptionChoice {
            value: "sha256".to_string(),
            label: "SHA-256".to_string(),
        },
        PluginOptionChoice {
            value: "md5".to_string(),
            label: "MD5".to_string(),
        },
    ],
}],
```

Read the resolved value in `run()`:

```rust
let algorithm = context
    .options
    .get("hashAlgorithm")
    .and_then(serde_json::Value::as_str)
    .unwrap_or("sha256");
```

Manifest defaults are merged with the values selected in the UI before the plugin runs.

## Execution order

The default execution order is `100`. Override it only when the plugin must run before other analyzers:

```rust
fn execution_order(&self) -> i32 {
    0
}
```

Filesystem producers such as Archive Extractor run early so their derived paths are available to normal analyzers. Prefer the default for independent analysis plugins.

## Related plugin manifests

`context.related_manifests` contains the manifests of other plugins selected in the same run. This is useful when a preparatory plugin needs to restrict its work to paths requested by downstream plugins.

Most plugins should not need this field.

## Error handling and persistence

Return `Err(String)` for a fatal plugin failure:

```rust
return Err(format!("Failed to parse '{}': {error}", path.display()));
```

The runner marks the job as failed and records the error. Collected artifacts, media, and derived paths are not committed.

For a recoverable per-file failure, log the problem and continue:

```rust
context.log("warn", format!("Skipped '{}': {error}", path.display()));
```

## Registration rules

- Plugin IDs must be unique.
- The registration instance must be static and implement `Send + Sync` through `BuiltinPlugin`.
- Give the constructor function and static instance unique Rust identifiers.
- Register exactly once.
- Add a `mod plugin_file;` declaration so Rust compiles the module.

The registration test verifies that built-in IDs are unique and that the expected built-ins were loaded.

## Validation

From `src-tauri` run:

```powershell
cargo fmt --all
cargo check
cargo test --lib
```

If the plugin changes frontend types or UI behavior, also run from the repository root:

```powershell
npx tsc --noEmit
bun run build
```

## Existing examples

- `src-tauri/src/plugins/builtin/archive_extractor.rs` contains archive extraction logic.
- `src-tauri/src/plugins/builtin/image_metadata.rs` contains byte-based media detection.
- `src-tauri/src/plugins/builtin/mod.rs` contains their `BuiltinPlugin` adapters and the shared context implementation.
