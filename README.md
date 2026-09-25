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

### 8. Local AI Assistant

The **Assistant** page (sidebar → Analysis) answers plain-language questions about a case, for example:

- `messages between john and jane march - jun 2025`
- `messages about money` or even `tranactiosn` (typos are corrected)
- `photos of vehicles`, `photos from july 2025`, `calls with jane`

People, dates, and result types are read from the question by a rule-based parser, and the filter it built is shown with every answer so it can be checked and edited. Content words are matched by meaning with **BGE-M3**, so "money" also finds "Sent 9,500 to the warehouse account". Weaker matches are kept out of the answer, under a collapsed **Possible links** section.

Click a result to go to it:

- a message opens its whole conversation, and from there the artifact or the source file
- a photo or video opens in Media
- a call or contact opens in Artifacts

**Everything runs on this machine.** Evidence is never sent anywhere. The AI models are downloaded once, after you confirm, and cached locally. The AI features come from three built-in plugins:

| Plugin | What it does | One-time download |
| --- | --- | --- |
| Media Tagging (AI) | Captions and tags photos (BLIP) so they can be searched | ~1.5 GB |
| Semantic Index (AI) | Indexes messages and captions for meaning search (BGE-M3) | ~2.3 GB |
| Hash Calculator | MD5 / SHA-1 / SHA-256 per file | none |

Plugins can be chosen when a datasource is added. To add more later, use **Plugins → Datasource Jobs → Add**.

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
- **Timeline analysis**
- **Geographical and location-based views**
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
