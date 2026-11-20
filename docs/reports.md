# HTML case reports

Open a case, select **Reports**, then **Generate report**. The modal lets you choose a title, examiner, notes, report sections and datasources. Select at least one section; evidence sections require at least one datasource when the case contains sources. The optional browser opening is enabled by default.

Available sections:

- Case information: case identity, reference, description and examiner.
- Evidence sources: selected datasources, imported paths and original image references where recorded.
- Artifact records: latest completed run for each datasource/parser, including every saved table row rather than the current UI page. Artifacts appear in named tables with entry counts, datasource/parser/source details, and individual labelled columns, inspired by iLEAPP's reports. Related records from the same source share a table. Parser column labels and timestamp types are retained; undeclared fields are included too. Nested objects and lists are shown as readable text instead of JSON blocks.
- Media inventory and stored hashes: file metadata, hashes already recorded in metadata, manual classifications and saved analysis results. Media binaries and thumbnails are not embedded.
- Timeline events: events derived from saved artifact timestamps. The timeline index is synchronized when this section is requested.
- Processing history and logs: saved plugin runs and log messages for selected datasources.

Reports are saved beside the case database in `reports/<report-id>/report.html`. Each self-contained HTML file includes its styles, a contents list and browser print styling. No external assets or network connection are required. Source values are escaped as text. Known generated/processing timestamps use UTC; original artifact values and timezone information are retained as recorded.

The companion `report.json` records the selection, generation time, exported row counts, size and SHA-256 of the HTML at generation. The Reports page lists these saved snapshots and offers **Open report** and **Show folder**. Generating another report creates a separate snapshot. Moving the whole case preserves report history because report paths are resolved relative to the current case database.

Generation streams a consistent SQLite snapshot into a private temporary directory and publishes it only after the HTML and manifest are complete. Failed generation removes the incomplete output. Stored source hashes are reported as recorded; report generation does not calculate or verify evidence hashes.

Generate a new report to use the current presentation; existing saved HTML snapshots keep their original contents.

Validation: `cargo test --offline --manifest-path src-tauri/Cargo.toml --lib reports::` covers datasource scope, latest-run selection, exports beyond UI pagination, labelled artifact tables, nested values, timestamps, HTML escaping, section selection, empty cases, persisted history, output digest and failed-generation cleanup.
