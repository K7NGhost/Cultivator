## Python Plugin Runtime

Python plugins run in an external worker process. The `python-plugins` Cargo
feature is enabled by default and has no native Python build dependencies.

For local development, run:

```powershell
bun run tauri dev
```

Install [uv](https://docs.astral.sh/uv/getting-started/installation/) on the machine
that runs Cultivator. Python itself does not need to be installed. Cultivator
finds uv on PATH or in its standard user installation directories; set
`CULTIVATOR_UV` to an executable path for a custom installation. The Plugins
page offers **Set up Python**, or setup happens automatically before execution.
For release packaging:

```powershell
bun run tauri build
```

Builds and installers do not download, link, or bundle CPython. At runtime, uv
downloads managed interpreters into `<app-data>/python/interpreters`, stores its
cache in `<app-data>/python/cache`, and creates environments under
`<app-data>/python/environments`. Initial setup needs network access; completed
environments are reused without uv or network access. Failed setup is retried on
the next run and reported in the plugin job log.

Existing `plugin.toml` manifests continue to work with Python 3.12 and no extra
dependencies. Plugins can declare their requirements:

```toml
python = ">=3.11"
dependencies = ["beautifulsoup4==4.14.3", "pillow>=12"]
```

Each plugin has an isolated environment keyed by its ID, Python requirement,
and dependency list. Changing requirements provisions a new environment; old
environments remain available until the app data is cleaned up. Pin dependencies
for reproducible analysis. Media previews use a separate environment with pinned
Pillow and pillow-heif packages and never modify original evidence.

Rust sends a versioned JSON request over stdin to a fresh worker for each plugin
job. The worker handles all matched files, preserves the `cultivator_api` and
context dictionary interfaces, and returns artifacts and logs as JSON. Python
stdout is redirected to diagnostics, progress arrives over stderr, and
cancellation terminates the worker. Plugin exceptions fail that job; later jobs
start new workers. Process isolation is not a security sandbox: scripts retain
the current user's filesystem and network permissions.

Run worker protocol tests with `bun run test:python`. Backend environment tests
are included in `cargo test`. Rust builds and tests require no installed Python.

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
