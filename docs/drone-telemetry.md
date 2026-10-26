# Drone telemetry

The built-in **Drone Telemetry** plugin reads supported DJI DAT V1/V3 logs and
saves case-owned conversions. Add a datasource containing `.dat` files, select
Drone Telemetry in Plugins, and run it. Open **Drone** in the sidebar to inspect
the results. The parser runs locally in Rust and does not require Python.

The workspace includes a flight library, street/satellite map, elapsed-time
playback, altitude/speed charts, sample instruments, controller events, a
searchable paged telemetry table, CSV export, and source/conversion hashes.
Selections in the table, timeline and charts update the map and instruments.
Results persist across case reopen and app restart. A failed rerun retains the
last completed results.

## Parser and map provenance

The DAT decoder (`src-tauri/src/drone/dat`) and MapLibre canvas/map utilities
were reused from the user-provided Samara repository at
`C:\Users\kevin\DevWork\Projects\Samara`, commit
`8743826e6195c548a93622347dabbd5cc9907f6b`. Samara identifies its decoder as
DatCon-based. Original module comments are retained. Cultivator adds built-in
plugin execution, cancellation checks, case-scoped persistence, artifact
summaries, CSV paging/export commands, and a workspace adapted to its theme.
Samara's parser tests are retained alongside plugin integration tests.

## Scope and interpretation

- Supports the aircraft clock identifiers and record layouts implemented by
  Samara, including its Phantom, Inspire, Mavic, Spark and Matrice variants.
  This is a focused decoder, not complete support for every DJI DAT format or
  every DatCon field. Unknown clock identifiers are rejected rather than guessed.
- Encrypted `LOGH` files are unsupported. Inputs are limited to 512 MiB, matching
  Samara. CRC-invalid packet candidates are excluded and recovery counts retained.
- Elapsed time is shown as elapsed time. GPS UTC, when decoded, remains in the
  telemetry table. Missing coordinates and instruments remain unavailable;
  there is no sample route in the production page.
- Coordinate source, ticks and byte offsets are retained. Altitude references
  remain explicit; an altitude is not automatically a height above ground.
- Full decoded telemetry is stored in a hashed CSV in the case's
  `artifacts/drone/<datasource>/<conversion-id>/` directory. Reopening validates
  its hash and row count. Source DAT files are read without modification.
- Map transfers are limited to approximately 20,000 GPS fixes, retaining both
  endpoints. Larger tracks are labeled as sampled; the complete route and
  telemetry remain in CSV. Summary distance is calculated from the full route.
- Street tiles use OpenFreeMap and satellite imagery uses Esri, as in Samara.
  The GPS route remains visible on a plain background when basemaps cannot load.
  Parsing, tables and exports work without a basemap connection. Attribution
  remains visible on the map.
- Controller-event previews retain Samara's 1,000-event limit. Unknown record
  layouts and other parsing limitations are reported in the flight's notes.

## Checks

```powershell
cargo test --offline --manifest-path src-tauri/Cargo.toml drone --lib
bun test tests/droneTelemetry.test.ts
bun run typecheck
bun run build
```

Parser fixtures cover CRC recovery, clock rollover, coordinate source selection,
missing GPS, CSV pagination and hash verification. Plugin tests cover overlapping
datasource paths, unsupported files, unchanged input bytes, case isolation and
visibility of the last successful run. Map tests cover invalid GPS, date-line
crossings and playback against recorded elapsed times.
