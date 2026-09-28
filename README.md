# Cultivator

Cultivator is a modern digital forensics application designed to expand upon the capabilities and workflows commonly found in tools such as Autopsy.

Built with **Rust** and **React**, Cultivator combines the performance, safety, and concurrency capabilities of Rust with a modern and responsive user interface.

## Why Cultivator?

Cultivator began as a personal project inspired by my experience working with digital forensics tools such as Autopsy, Cellebrite, and Magnet AXIOM.

Each of these platforms is highly capable, but through regular use I encountered workflow and development limitations that I wanted to explore solving in a different way.

Some of the primary motivations behind Cultivator include:

- **Autopsy** — Extending and developing modules can involve a relatively complex development workflow.
- **Cellebrite** — Large processing jobs can be resource-intensive, with limited control over exactly which analysis operations are performed.
- **Magnet AXIOM** — Comprehensive processing can also be resource-intensive, and some workflows could benefit from a more flexible and customizable interface.

Cultivator is intended to provide a lightweight, extensible, and developer-friendly alternative for experimenting with modern digital forensics workflows.

## What Makes Cultivator Different?

Cultivator focuses heavily on performance, extensibility, and giving examiners greater control over how evidence is processed.

### 1. Modern Architecture

Cultivator is built primarily with Rust and React, providing a modern foundation that is intended to make the application easier to maintain, extend, and improve over time.

### 2. Built-In Search and Grep

Examiners can search directly through evidence from within the application without necessarily waiting for a complete search index to finish building.

This can be especially useful when performing targeted searches during an investigation.

### 3. Integrated Media Analysis

Image and video analysis functionality is integrated directly into the media gallery, allowing media processing and review to remain part of the same workflow.

### 4. Python Artifact Plugins

Cultivator supports lightweight Python plugins for artifact extraction and processing.

The plugin system is designed to make creating new artifact parsers as straightforward as possible without requiring developers to understand the entire application architecture.

### 5. Dark Mode

A native dark interface is available for examiners who prefer working in low-light environments or during extended analysis sessions.

### 6. Developer-Friendly Plugin System

Extensibility is a major focus of Cultivator.

The plugin architecture is designed to minimize the amount of boilerplate required to create new forensic artifact parsers, analysis modules, and other functionality.

### 7. File Hashing

The built-in **Hash Calculator** plugin (on by default) records MD5, SHA-1, and SHA-256 for every file for chain-of-custody verification. Hashes show in the Files view, and **Copy Hashes** copies them from a file's context menu.

**Hash sets** (**Analysis → Hash sets…**) are lists of known hashes that every case is checked against: known-bad lists (Project VIC / CAID exports, a lab's own lists) flag files, and known-good lists such as NSRL mark files that are usually safe to set aside. Plain lists, CSV (including NSRL's format), hashdeep output and JSON exports can be imported; they are stored on this computer once, for all cases. Matches appear under **Files → Hash Set Hits**.

**Extension Mismatch** (**Files → File Views**) lists files whose content contradicts their name, such as a `.jpg` that is really a zip, by reading each file's signature.

### Tags and notes

Files and photos can be tagged (Bookmark, Follow Up, Notable, Project VIC categories, or any custom tag) with a note, from the Files page or a photo's details in Media. Tags are stored with the case and listed under **Files → Tags**.

### Timeline and map

The **Timeline** (sidebar → Analysis) puts every dated message, call, photo and video on one list, with a chart of activity over time (click a bar to zoom in), an activity-by-hour chart for pattern of life, and filters by type, date and text. Click an event to open it.

The **Map** plots photos by their GPS coordinates, plus any location artifacts from plugins. The world map is built into the app, so viewing it never sends evidence locations to an online map service. Click a point to see the photo and open it in Media.

### 8. Local AI Assistant

The **Assistant** page (sidebar → Analysis) answers plain-language questions about a case, for example:

- `messages between john and jane march - jun 2025`
- `messages about money` or even `tranactiosn` (typos are corrected)
- `photos of vehicles`, `photos from july 2025`, `calls with jane`

People, dates, and result types are read from the question by a rule-based parser, and the filter it built is shown with every answer so it can be checked and edited. Content words are matched by meaning with **BGE-M3**, so "money" also finds "Sent 9,500 to the warehouse account". Weaker matches are kept out of the answer, under a collapsed **Possible links** section.

Questions about behaviour, such as `people trying to be discreet`, `threats` or `fraud`, name an idea the messages never state. For these, Cultivator uses built-in **investigator themes** (secrecy, destroying evidence, threats, fraud, drugs): typical wording of each behaviour ("Delete these texts", "Use the burner phone") is searched alongside the question, and the theme used is shown on the answer.

A second local model, **Qwen3-Reranker-0.6B**, then double-checks the best matches (a *precision check*): it reads the question and each match together and drops the ones it judges unrelated. Vector search alone can't tell that "The keys is on the counter." has nothing to do with money, and on a large phone such filler crowds the top of the list: on a 100,000-message test phone, 20 of the top 25 matches for "money" were filler and the precision check removed all 20 while keeping every real transfer. Results appear immediately and are refined when the check finishes (under a second on a GPU, a few seconds on a CPU); confirmed matches carry a check mark. The **Precision check** chip on an answer shows how many matches were removed; remove the chip to see every match the vector search found.

Click a result to go to it:

- a message opens its whole conversation, and from there the artifact or the source file
- a photo or video opens in Media
- a call or contact opens in Artifacts

**Everything runs on this machine.** Evidence is never sent anywhere. The AI models are downloaded once, after you confirm, and cached locally. The AI features come from three built-in plugins:

| Plugin | What it does | One-time download |
| --- | --- | --- |
| Media Tagging (AI) | Captions and tags photos (BLIP) so they can be searched | ~1.5 GB |
| Semantic Index (AI) | Indexes messages and captions for meaning search (BGE-M3), and brings the Assistant's precision check (Qwen3-Reranker-0.6B) | ~3.5 GB |
| Hash Calculator | MD5 / SHA-1 / SHA-256 per file | none |

Plugins are installed once and every installed plugin is ticked when a datasource is added (**Select all** / **Clear** change that in one click). A plugin that isn't wanted in a case can be switched off in the **On** column of **Plugins → Installed Plugins**; it stays installed, and the case remembers the choice. To add more to an existing datasource, use **Plugins → Datasource Jobs → Add**.

On a computer with an NVIDIA GPU (GTX 10-series / Quadro P or newer, driver 528 or newer), the AI plugins and the Assistant run on the GPU, which captions and indexes many times faster than the CPU. Cultivator picks the PyTorch build that matches the card and driver: CUDA 13.0 or 12.8 for current drivers, CUDA 12.6 for the older certified drivers and Pascal/Maxwell cards common in lab workstations (these older cards run in float32, where they are faster). A first install on such a machine downloads the GPU build of PyTorch automatically; an existing CPU install can be switched with **Use GPU** on the Plugins page. Without a usable GPU (none, an old driver, or under 1.5 GB free), everything runs on the CPU. The models stay loaded between batches and start loading as soon as a run begins, so their load time overlaps the other plugins.

Large sources are handled incrementally: re-runs keep the captions of unchanged photos and only index new text, the index is saved batch by batch (a cancelled run resumes where it stopped), and the Assistant keeps the case's index in memory between questions.

## Security and Lab Use

- **Evidence is never modified.** SQLite databases found in evidence are opened from a private copy (with their `-wal`/`-shm` files, so uncommitted WAL data is still visible) under `%TEMP%\cultivator-sqlite-copies`, which is deleted when Cultivator starts and exits. Opening a WAL database in place, even read-only, rewrites its `-shm` file.
- **Evidence can't run code.** Executables, scripts, installers and shortcuts (`.exe`, `.lnk`, `.ps1`, `.msi`, …) found in evidence can't be launched from Cultivator; use the Text/Hex preview or Reveal in folder.
- **Local only.** Nothing is uploaded. The only network use is the one-time, confirmed download of AI packages and models; once a model is on disk it runs with the network switched off (`HF_HUB_OFFLINE`), and telemetry is disabled. The app loads no remote scripts, and a strict Content Security Policy blocks any that evidence content might try to inject.
- **Air-gapped labs.** Copy `%APPDATA%\com.k7nghost.cultivator\models` from a connected machine, and point `CULTIVATOR_WHEELHOUSE` at a folder of wheels made with `pip download torch==2.14.0 transformers==5.17.0 pyspellchecker==0.9.0 -d wheels` (add `--index-url https://download.pytorch.org/whl/cu130` for GPU machines). pip then never contacts an index.
- **Reproducible.** Model revisions and package versions are pinned, and each AI plugin's log records the device and model revision it ran with (e.g. `on NVIDIA GeForce RTX 5070 Ti Laptop GPU with BAAI/bge-m3@5617a9f`). The precision check's model is pinned the same way and reads its weights from safetensors only (no pickle).
- **Open licenses.** BLIP (BSD-3-Clause), BGE-M3 (MIT) and Qwen3-Reranker-0.6B (Apache-2.0).
- **Plugins are code.** Python plugins run with the examiner's permissions; install only plugins from a trusted source.
- **Signing.** Release builds are not code-signed yet. Labs that require signed software (and Windows Smart App Control) need a signed installer.

## Getting Started

Requirements: [Rust](https://rustup.rs/), [Bun](https://bun.sh/), and on Windows the Visual Studio C++ Build Tools.

```powershell
bun install
bun run tauri dev    # sets up the bundled Python runtime and starts the app
```

Run the tests:

```powershell
bun test                                  # frontend (assistant parser and engine)
cd src-tauri; cargo test --lib            # Rust (plugins, hashing, semantic search)
```

See [GENERAL_GUIDE.md](GENERAL_GUIDE.md) for the Python plugin runtime and [RUST_PLUGIN_GUIDE.md](RUST_PLUGIN_GUIDE.md) for writing built-in plugins. To try the features on fictional data, see [test-data/demo](test-data/demo/README.md).

## Planned Features

Cultivator is still under active development. Planned features include:

- **Expanded local AI assistance**, such as video tagging and summaries
- **The Sleuth Kit integration** through a Rust-compatible wrapper or interface
- Continued **UI and performance improvements**
- Standardized **artifact types** for building specialized artifact visualizations
- **Reports**: exporting tagged items with hashes and examiner notes
- Additional forensic parsers and analysis modules
- Expanded plugin capabilities

## Project Goals

The long-term goal of Cultivator is not simply to replicate existing forensic platforms, but to explore how a modern forensic application can provide:

- Faster and more selective evidence processing
- A cleaner and more responsive examiner workflow
- Simple plugin and artifact development
- Modern visualization capabilities
- Greater control over which analysis operations are performed
- An architecture that can continue evolving as forensic workflows change

Cultivator is an ongoing personal project and will continue to evolve as new forensic workflows, artifact types, and analysis capabilities are implemented.

Current Statistics - researching
