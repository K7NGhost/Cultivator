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

The built-in **iOS Artifacts**, **Android Artifacts**, and **Vehicle Artifacts** plugins bundle the complete
iLEAPP, ALEAPP, and VLEAPP artifact catalogs. See [mobile and vehicle artifact plugins](docs/mobile-artifact-plugins.md)
for supported inputs, runtime setup and validation.

The plugin system is designed to make creating new artifact parsers as straightforward as possible without requiring developers to understand the entire application architecture.

### 5. Dark Mode

A native dark interface is available for examiners who prefer working in low-light environments or during extended analysis sessions.

### 6. Developer-Friendly Plugin System

Extensibility is a major focus of Cultivator.

The plugin architecture is designed to minimize the amount of boilerplate required to create new forensic artifact parsers, analysis modules, and other functionality.

## Evidence importing

Cultivator uses the sibling [Inodex](../inodex) Rust library to import logical
files/directories and supported disk images and filesystems. Image imports index
the filesystem; case-owned working files are created only when plugins or viewers
need local paths. Rust metadata and hashing consumers can read directly through
Inodex, with original source metadata recorded separately.

See [Inodex integration](docs/inodex-integration.md) for setup, importing,
extraction/storage behavior and current limits, and the
[media gallery benchmark](docs/media-gallery-benchmark.md) for the measured baseline.

## Planned Features

Cultivator is still under active development. Planned features include:

- **Local AI assistance** for helping examiners analyze and interpret forensic data
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
