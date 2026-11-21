# Case search indexing

Settings → Search indexing controls a native Tantivy worker for the open case.

- **Adaptive** (default): checks periodically, yielding until 12 seconds after the latest click, keypress or scroll.
- **Background**: checks continuously at the configured interval.
- **Manual**: never starts a pass automatically. **Index now** requests one pass in any mode and bypasses Adaptive yielding.

The default discovery interval is 60 seconds, configurable from 10 to 3,600 seconds. Closing or switching cases cancels the current pass at a checkpoint. Changing indexing settings also cancels the current pass; automatic modes start another pass, while Manual waits for a request.

Reopening a completed case restores its saved coverage immediately and waits the configured interval before its next automatic check. Opening a case does not request a rebuild. A new case, missing index/checkpoints, unavailable progress summary or changed content-size setting still needs an indexing/checking pass. Index now remains available to request an immediate check.

## Indexed information

The worker reconciles three stages in order:

1. Filesystem metadata: paths, sizes and timestamps from data source roots, including virtual evidence manifests.
2. Artifacts: individual table rows and ordinary result payloads from the latest completed job for each data source/plugin pair. Incomplete jobs and internal file metadata/text samples are excluded.
3. File contents: UTF-8 text and UTF-16 text with a byte-order mark. Binary files and files over the configured limit are excluded. The default limit is 8 MiB, configurable from 1 to 32 MiB. Artifact records over 256 KiB are excluded before transferring their bodies from SQLite.

PDF/Office extraction and OCR are outside this indexing step. Optional BGE Large embeddings and LanceDB retrieval are described in [Semantic retrieval](vector-search.md); evidence-grounded Qwen answers are available through [Case chat](case-chat.md).

## Search views

The Search page has **Indexed** and **Grep** tabs, using the same navigation pattern as the map workspaces. Indexed is the default; `#/search?view=grep` opens Grep directly. Each tab retains its query and results when switching views.

Indexed search queries the latest committed Tantivy snapshot without scanning evidence. It supports category filters, phrases in quotes and Boolean queries, showing relevance-ranked text chunks with their source path and record identity. Pages contain 50 chunks; queries must be narrowed beyond the first 10,050 matches. Index now and the indexing settings link are available in this view. Results refresh when you submit or page through a query.

Artifact and metadata results display readable summaries and labeled fields instead of serialized JSON. Nested values retain their parent labels, known filesystem sizes and timestamps are formatted for display, and large integer IDs keep their exact digits. Truncated records are identified as excerpts. Original indexed text remains available in a collapsed section, while file content is shown as recorded text. This presentation works with existing indexes and does not require reindexing.

Grep retains the existing file scanning interface, regex/case options and text/hex previews. Indexed results only cover data processed so far and respect the content exclusions above.

Each Tantivy document stores a stage, source path, stable group identity, chunk ordinal and text. Artifact identities retain the result ID and original table row index. Text uses chunks of at most 2,048 UTF-8 bytes with about 256 bytes of overlap, preserving character boundaries for future retrieval.

## Performance and persistence

Each case database has a sibling directory named `<database filename>.tantivy-v1`. It contains the Tantivy index, a SQLite checkpoint ledger and the last successful progress summary. Settings are stored in the application's preferences and apply across cases.

One worker feeds a single Tantivy indexing thread with a 32 MiB writer buffer. Work is checkpointed in batches of 128 records, with cooperative yields and cancellation checks. Evidence manifests are iterated without cloning their full record list; artifact tables use a single streaming `json_each` cursor rather than repeated offset queries. Content candidates are probed with an 8 KiB prefix before reading further. Original evidence is read directly through the evidence reader without extracting whole files.

Unchanged size/metadata fingerprints skip content reads and document replacement. A completely unchanged pass does not commit new Tantivy segments. This deliberately avoids repeatedly hashing whole files; files changed while retaining identical filesystem metadata require reimporting or rebuilding the index to guarantee detection.

Changes replace all chunks for their stable group. Deletions are reconciled only after a stage scans successfully. Tantivy commits precede SQLite checkpoints: an interrupted batch replays delete-before-add operations, avoiding duplicate documents. If the search index is missing, its old checkpoints are discarded and documents are rebuilt on the next pass.

Progress bars show processed records, exclusions and the last successful scan. Initial discovery remains indeterminate until totals are known. Later checks retain the last completed coverage and show a separate checked count instead of resetting every stage to zero. Checks still examine source metadata and artifact fingerprints to detect additions, modifications and deletions; unchanged documents are reused. Completion includes examined exclusions, so 100% does not mean every file has searchable text. Unreadable content is retried on the next pass; scan failures remain visible in Settings.
