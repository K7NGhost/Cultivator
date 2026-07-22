# Cultivator

Cultivator is a Tauri, React, and TypeScript forensics application.

## Python Plugin Runtime

Python plugins are compiled behind the Tauri Cargo feature `python-plugins`.

For local development, run:

```powershell
bun run tauri dev
```

The Tauri wrapper enables the `python-plugins` Cargo feature, installs or
refreshes the redistributable CPython runtime under `src-tauri/python-runtime`,
and configures `PYO3_PYTHON`, `PYTHONHOME`, and platform loader paths before
starting Tauri. The setup also installs the pinned Pillow and pillow-heif
packages used to create browser-compatible previews without modifying original
evidence files. For release packaging, build with the Python feature enabled:

```powershell
bun run tauri build
```

The release script downloads a shared `python-build-standalone` runtime for the
current OS and CPU, installs it under `src-tauri/python-runtime`, sets
`PYO3_PYTHON`, `PYTHONHOME`, and platform loader paths for the build, and
includes that runtime as a Tauri resource. It uses CPython 3.12 by default; set
`CULTIVATOR_PYTHON_VERSION` before running the script to choose another stable
minor version.

To install or refresh the runtime without running a release build:

```powershell
bun run python:runtime
```

PyOxidizer remains optional for build setups that support static embedding:

```powershell
python -m pip install pyoxidizer
bun run python:embed
```

Then build with `PYO3_CONFIG_FILE` pointing at
`src-tauri\target\pyembed\pyo3-build-config-file.txt`.

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
