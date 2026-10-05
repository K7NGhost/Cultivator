# CI, performance, and releases

## Workflows

- `ci.yml` runs on PRs, main, merge queues, and manual dispatch. It checks workflow
  syntax, TypeScript, Bun tests, production frontend builds, Rust formatting,
  Clippy, and Rust tests. Rust is checked both with and without `python-plugins`.
  Native builds verify Windows, Linux, macOS Intel, and macOS ARM with the bundled
  CPython runtime. PRs compile the application without generating installers.
- `performance.yml` benchmarks SQLite header parsing, MP4 container metadata,
  and preview serialization through CodSpeed CPU simulation. The existing
  asynchronous text-preview benchmark runs separately with Criterion, and its
  timing reports are uploaded as workflow artifacts. Those I/O timings are
  diagnostic; shared-runner noise makes them unsuitable as a strict merge gate.
- `release.yml` runs on `v*` tags. It first validates versions and calls CI's
  quality checks, then creates a draft release and builds installers on all four
  native runners. The release is published only after every packaging job passes.
  Failed packaging leaves a draft that can be resumed by rerunning failed jobs.
  Already-published tags cannot be rebuilt through a fresh workflow run.

The shared `setup-project` action pins Bun 1.3.8, Node 22, and Rust 1.93.0, uses
frozen dependency locks, installs Tauri's Linux prerequisites, and caches Bun,
Cargo, and the platform-specific Python runtime. Python environment variables
and library paths are exported for Cargo tests as well as Tauri builds. Native
macOS runners match the Python runtime architecture, avoiding cross-linking
Intel Rust binaries against ARM Python libraries.

There is no frontend ESLint setup in this repository. Frontend checks use the
existing strict TypeScript rules and Bun tests instead of introducing another
test runner or presenting a typecheck as an ESLint check.

## Enable performance protection

1. Install the [CodSpeed GitHub integration](https://codspeed.io/docs/integrations/providers/github)
   for this repository and run Performance on main to establish a baseline.
2. Set the desired regression threshold in CodSpeed. The benchmark job uses
   [OIDC authentication](https://codspeed.io/docs/integrations/ci/github-actions/configuration);
   no static token is configured. Public fork PRs can use tokenless uploads when
   GitHub does not issue an OIDC token. Private fork policies may require a
   maintainer-triggered run with appropriate repository access.
3. In GitHub branch protection or a ruleset for main, require `Workflow Checks`,
   `React Checks`, `Rust Format`, `Rust Checks (core)`, `Rust Checks (python)`,
   `Build (Windows)`, `Build (Linux)`, `Build (macOS Intel)`, `Build (macOS ARM)`,
   and CodSpeed's `CodSpeed Performance Analysis` check. Wait until the checks have
   run before selecting them. The workflow's successful upload is separate from
   CodSpeed's regression decision.

Workflow files do not install the GitHub App, change branch protection, or set a
CodSpeed regression threshold; those are repository/account settings.

The larger 100 MiB text-preview workload runs nightly, on PRs carrying the
`performance` label, or when Performance is dispatched with `large: true`. The
nightly schedule is 06:23 UTC (2:23 a.m. Eastern during daylight saving time,
1:23 a.m. during standard time). Small fixtures are 4 MiB each; eight of them
exercise map-cache misses. The large case measures a warm map. Fixtures are
generated in a temporary directory outside measured work and contain synthetic
log records. On Windows, cached memory maps may keep temporary fixtures alive
until process exit.

CodSpeed simulation deliberately covers synchronous CPU work. For reliable I/O
and end-to-end performance budgets, add CodSpeed dedicated macro runners and
walltime measurements; do not infer disk latency from CPU simulation. Add
benchmarks for real indexing, hashing, and extraction APIs as those public core
modules become available. The workflow does not claim to benchmark the Tauri GUI
or the hypothetical APIs in the original proposal.

## Create a release

Update the version in `package.json`, `src-tauri/Cargo.toml`, and
`src-tauri/tauri.conf.json`, and refresh both frontend locks and `Cargo.lock` if
their package versions changed. Commit the changes and tag that commit, for
example `v0.2.0`. Pushing the tag starts Release. A tag such as `v0.2.0-beta.1`
must match the same prerelease version in all three files and creates a GitHub
prerelease.

GitHub Actions must be enabled. Release jobs use the repository's automatically
provided `GITHUB_TOKEN` with `contents: write`. No personal access token is needed.
Installers contain the bundled Python runtime and enable `python-plugins` through
the existing `bun run tauri` wrapper. The workflow does not publish updater
metadata because this application has no updater configured.

## Optional signing

Without signing secrets, packaging produces unsigned installers/bundles. When
signing is configured, invalid or incomplete credentials fail packaging rather
than silently falling back to unsigned output.

For macOS, configure `APPLE_CERTIFICATE` (base64-encoded `.p12`),
`APPLE_CERTIFICATE_PASSWORD`, and `APPLE_SIGNING_IDENTITY`. For notarization, also
configure `APPLE_ID`, `APPLE_PASSWORD` (an app-specific password), and
`APPLE_TEAM_ID`. Tauri handles signing/notarization using these environment
variables. See [Tauri macOS signing](https://v2.tauri.app/distribute/sign/macos/).

For an existing Windows signing certificate with an exportable private key,
configure `WINDOWS_CERTIFICATE` (raw base64-encoded `.pfx`, without PEM headers)
and `WINDOWS_CERTIFICATE_PASSWORD`. The release runner imports it into its
temporary user's certificate store and creates a Tauri configuration overlay
with its thumbprint and SHA-256 timestamping. Certificates backed by a hardware
token or a cloud signing service need their issuer's integration/custom signing
command instead; the PFX setup does not cover those providers. See
[Tauri Windows signing](https://v2.tauri.app/distribute/sign/windows/).

## Local checks

```text
bun install --frozen-lockfile
bun run typecheck
bun run test
bun run build
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets --no-default-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked --no-default-features
cargo test --manifest-path src-tauri/Cargo.toml --locked --benches --no-default-features
cargo bench --manifest-path src-tauri/Cargo.toml --locked --bench file_format
cargo bench --manifest-path src-tauri/Cargo.toml --locked --bench text_preview
```

All-feature Cargo checks require the bundled Python environment. The CI setup
script exports it to GitHub's environment files; `bun run tauri` configures it
automatically for native builds. To run the large benchmark locally, set
`CULTIVATOR_BENCH_LARGE=1` before invoking the text-preview bench. Install
`cargo-codspeed` 5.0.1 to build the CPU suite with `cargo codspeed build --bench
file_format --no-default-features --locked` from `src-tauri`.
