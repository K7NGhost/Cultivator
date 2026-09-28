# What this branch adds to `main`

Branch: `feature/fast-plugins-and-assistant-fixes`, compared with `main` (`ed59a9c`).

It builds on `feature/semantic-assistant-bge-m3` (commits `d6a88b8` and `0c0b560`) and adds the work on top of it. Together they give Cultivator:

- file hashing
- local AI photo captioning
- meaning-based search with a precision check
- a plain-language case Assistant
- per-case plugin control
- GPU acceleration that falls back to the CPU
- evidence-integrity and security hardening for lab use
- the speed to handle a 100,000-message phone

Everything runs on the examiner's machine. Evidence is never uploaded, and the only network use is a one-time model download that the examiner approves.

## At a glance

Measured on this project's test phones: RTX 5070 Ti laptop GPU, 12-thread CPU. The CPU-only rows used the same machine with the GPU switched off.

| | `main` | This branch |
| --- | --- | --- |
| File hashes (MD5 / SHA-1 / SHA-256) | none | every file, one streaming pass |
| Photo captions and tags | none | BLIP, on-device |
| Search by meaning | none | BGE-M3, plus a precision check by Qwen3-Reranker |
| Plain-language questions | none | Assistant page |
| All plugins, demo phone (GPU) | n/a | 17.5–21 s (about 70 s before the speed work) |
| All plugins, demo phone (CPU only) | n/a | 27 s |
| All plugins, 100,000 messages + 1,012 photos (GPU) | n/a | 115 s (229 s before the speed work) |
| Semantic index for 38,546 texts (GPU) | n/a | 34 s (138 s before) |
| Assistant question on the 100,000-message phone | n/a | 4.5 s first, 0.5–0.6 s after |
| "messages about money" on the 100,000-message phone | n/a | exactly the 5 real transfers (25 before the precision check, 20 of them filler) |
| Opening an evidence SQLite database | changes its `-shm` file | evidence byte-for-byte unchanged |
| Search indexing, CPU only (benchmark, 256 messages) | n/a | 40 texts/s on CPUs with native bfloat16, 21 on others |
| Memory for the Assistant's index | n/a | ~1.3 KB per indexed item (8-bit vectors) |
| Tests (passing) | 15 frontend, 41 Rust | 51 frontend, 64 Rust |

Plugin times were measured with the GPU at full power. Later runs on the same laptop took about 1.8x longer while its power settings capped the GPU at about 33 W. Answers were the same.

## 1. New features

### Hash Calculator (built-in plugin, on by default)

- Records MD5, SHA-1 and SHA-256 for every file in one streaming pass, into a `file_hashes` table, for chain-of-custody verification.
- Hashes show in the Files list and the file preview. **Copy Hashes** is in the file context menu.

### Media Tagging (AI) (built-in plugin)

- Captions every photo on-device with BLIP (`Salesforce/blip-image-captioning-base`) and derives keyword tags. The Media page shows captions and tags, and gets a search box.
- The model stays loaded in a long-lived worker. Photos are processed in batches: 16 on a GPU, 4 on a CPU.
- Large photos are decoded at reduced size, and EXIF rotation is respected. HEIC photos are read when `pillow-heif` is installed.
- Re-runs keep the captions of photos whose size and modification time haven't changed ("Kept existing captions for N unchanged images").

### Semantic Index (AI) (built-in plugin)

- Embeds messages and photo captions with BGE-M3 (`BAAI/bge-m3`) into a per-case `semantic_embeddings` table.
- Each vector is keyed by a hash of its text, so unchanged or repeated text is never embedded twice.
- Saves batch by batch, so a cancelled run resumes where it stopped.
- Waits for the message parsers of the same run to finish before indexing (see Bug fixes).
- Also downloads the Assistant's precision-check model.

### Assistant (Analysis → Assistant)

- **Questions become a visible, editable filter.** For example: "messages between john and jane march - jun 2025", "photos of vehicles", "calls with jane". People, dates, and result types (messages, calls, contacts, photos, videos) are read by a rule-based parser. The filter is shown as chips that can be removed or changed.
- **Dates are understood the way people write them:**
  - Typos in longer month names are corrected ("apirl" → April). People's names in the case and short words are never turned into months.
  - "between april and march" means March to April.
  - A range covering 11+ months is read in calendar order, with a note.
  - "before", "after", "since" and "until" work without a year.
  - Day-level ranges without a year are supported.
  - Reversed explicit years are swapped back.
- **Search by meaning.** Content words match exactly or by meaning, so "money" also finds "Sent 9,500 to the warehouse account".
  - Typos are corrected with pyspellchecker ("tranactiosn" → "transactions"). Both spellings are searched, and the better score is kept.
  - Confident matches form the answer. Weaker ones go into a collapsed **Possible links** section.
- **Precision check.** A second local model, Qwen3-Reranker-0.6B, reads the question together with each of the best matches and drops the ones it judges unrelated. The details are in [section 4](#4-precision-check).
  - Results appear immediately and are refined when the check finishes: under a second on a GPU, a few seconds on a CPU.
  - Confirmed matches show a check mark.
  - A **Precision check (removed N)** chip shows how many matches it removed. Removing the chip shows everything the vector search found, and the check can be turned back on.
- **Click-through.**
  - A message opens its whole conversation with the hit highlighted, with links to the artifact and the source file.
  - Photos and videos open in Media. Calls and contacts open in Artifacts.
  - This works through a small reveal helper (`src/app/reveal.ts`).
- **Fast on large cases.**
  - The case's search index stays in memory between questions.
  - The model starts loading when the page opens.
  - Answers are capped to the 25 best distinct matches, plus 100 possible links per group.
  - A message repeated hundreds of times counts once.

### Plugin workflow

- **Installed once, used everywhere.** Every installed plugin is ticked when a datasource is added, and the case remembers the last selection.
- **Select all** and **Clear** buttons are in the datasource wizard and the Add Plugins dialog.
- **Per-case on/off switches** are in the **On** column of Plugins → Installed Plugins, stored in a `case_plugin_settings` table. Switching off (instead of uninstalling) keeps the plugin available to other cases.
- **Add plugins later.** Plugins → Datasource Jobs → **Add** adds plugins to an existing datasource and can run them right away.
- **One download prompt.** A shared consent dialog lists what each AI plugin still needs to download (Media Tagging ~1.5 GB; Semantic Index plus the precision check ~3.5 GB, or ~1.2 GB if only the precision check is missing). On NVIDIA machines it mentions the ~3 GB GPU build of PyTorch.
- **AI: GPU / CPU badge** on the Plugins page. **Use GPU** switches an existing CPU install to the CUDA build of PyTorch.

### Datasource wizard safety

- A datasource that contains the case folder is blocked, with an explanation. Otherwise the case's own files would be processed as evidence.

## 2. Security and forensic-lab readiness

| Area | What changed |
| --- | --- |
| **Evidence integrity** | SQLite databases found in evidence are opened from a private copy (with their `-wal`, `-shm` and `-journal` files) under `%TEMP%\cultivator-sqlite-copies`. Opening a WAL database in place, even read-only, rewrites its `-shm` file, and `immutable=1` would hide uncommitted WAL rows. A test confirms that WAL-only rows are visible and the evidence hashes are unchanged. The copies are deleted when Cultivator starts and exits. |
| **No code execution from evidence** | "Open with Windows" is blocked for 37 risky types, case-insensitive: `.exe`, `.lnk`, `.ps1`, `.bat`, `.msi`, `.js`, `.hta`, `.dll`, `.url` and others. The examiner is pointed to the Text/Hex preview or Reveal in folder instead. |
| **No remote code** | Removed a `<script src="http://localhost:8097">` (React DevTools) from `index.html`: any local process listening on that port could have injected code. |
| **Content Security Policy** | A strict CSP: scripts only from the app, no plugins or frames, and network access limited to Tauri IPC. This blocks script injection through evidence content. End-to-end runs visit every page with 0 violations. |
| **Local only** | Once a model is on disk, its worker runs with `HF_HUB_OFFLINE=1`, and Hugging Face telemetry is disabled. The Assistant never downloads anything: models come only from an approved plugin run. |
| **Air-gapped labs** | Copy the `models` folder from a connected machine. Set `CULTIVATOR_WHEELHOUSE` to a folder of pre-downloaded wheels, and pip never contacts an index. |
| **Reproducible** | Model revisions are pinned: BLIP `82a3776`, BGE-M3 `5617a9f`, Qwen3-Reranker `e61197e`. Python package versions are pinned. Each AI plugin's log records the device and model revision it ran with. |
| **Safe model loading** | The precision-check model loads from safetensors only, never pickle. Downloads are ordered so an interrupted download is retried rather than half-used. |
| **Licenses** | BLIP (BSD-3-Clause), BGE-M3 (MIT), Qwen3-Reranker-0.6B (Apache-2.0). |
| **Lifecycle** | Every model worker is shut down when the app exits. When idle, the captioning worker stops after 5 minutes and the Assistant's two models after 30. |
| **Window** | Proper title, starts maximized, with a minimum size. |

## 3. Speed and large cases

- **Long-lived model workers.** BLIP, BGE-M3 and the reranker each load once per session in their own Python process. The first version of this branch started BLIP once per batch. Models start loading as soon as a run begins, so load time overlaps the other plugins.
- **GPU acceleration on lab hardware, with CPU fallback.** Cultivator picks the PyTorch build from both the NVIDIA driver and the card's architecture (read from `nvidia-smi`):
  - CUDA 13.0 for driver R580+ on Turing or newer cards.
  - CUDA 12.8 for R570+, needed for RTX 50-series.
  - CUDA 12.6 for the older certified drivers (R528+) and the Maxwell/Pascal cards (GTX 10-series, Quadro P) that many lab workstations keep.

  Cards from 2017 on (Volta and newer) run the models in half precision. Older cards run float32, where they are much faster. Each model still runs a quick self-test and needs 1.5 GB of free GPU memory (3 GB in float32); otherwise it runs on the CPU. `CULTIVATOR_AI_DEVICE=cpu` forces the CPU.
- **Bigger, smarter batches.**
  - Texts are sorted by length to avoid padding. Batch sizes: embeddings 128 on a GPU and 16 on a CPU; index requests 256 and 32.
  - If the GPU runs out of memory, the worker retries one item at a time.
- **Binary vector transport.** Vectors come back as packed float32 instead of JSON numbers. Formatting millions of floats as text cost more than computing them.
- **Incremental work.** Unchanged photos keep their captions, already-indexed text is skipped, and the index is saved per batch.
- **Assistant at scale.**
  - The corpus is cached per case and reloaded only when plugin results change.
  - Ranking caps stop a 100,000-message case from returning 5,000+ "matches".
  - Only one copy of a repeated text can be a confident match.
  - The precision check reads only the 25 best distinct texts per group, so its cost doesn't grow with the case.
- **Precision check on a CPU.** The prompt's shared start (instruction and question) is computed once per question and reused for every candidate. Only the "yes"/"no" rows of the output layer are computed. Together that's 16 s → 4.3 s for 40 candidates on a CPU, with identical scores.
- **bfloat16 on capable CPUs.** Search indexing and the precision check run in bfloat16 on CPUs with native bfloat16 instructions (AVX512-BF16 or AMX, e.g. recent AMD Ryzen). Other CPUs keep float32, where emulated bfloat16 would be slower. The device name in the logs says which ran.
  - Indexing: 21 → 40 texts/s, with vectors 99.99% identical.
  - Precision check: 4.3 → 2.3 s for 40 candidates, with scores within 0.1.
  - Captioning stays float32, because BLIP's captions changed in bfloat16.
- **8-bit model weights were tested and rejected.** They were only 1.3–2.3x faster, and they changed results too much: search vectors fell to 0.92 similarity, precision-check scores moved by up to 8.5, and captions got worse.
- **8-bit vectors in memory.** The Assistant keeps each search vector as 8-bit integers plus one scale. That's about 1 KB instead of 4 KB per item, and scores move by about 0.0003. The case database keeps the full float32 vectors.

## 4. Precision check

Vector search compares meanings approximately. On a large phone, short filler lines can rank near the top for almost any topic: on the 100,000-message test phone, 20 of the top 25 distinct matches for "money" were lines like "The keys is on the counter."

The precision check fixes this:

- **Model:** `Qwen/Qwen3-Reranker-0.6B` (1.2 GB, Apache-2.0), in its documented yes/no prompt format. It is instruction-following, so it understands indirect matches such as a transfer for "money".
- **Why not `BAAI/bge-reranker-v2-m3`:** it was tested first and rejected. With keyword queries it scored the real transfers ("Sent 9,500 to the warehouse account.") near 0% and would have removed them.
- **Rule:** a conservative veto. The check reads each group's 25 best distinct matches. Matches it scores below −7 log-odds are dropped, and so are matches it didn't read. Nothing is promoted.
  - Calibration: every real match on the demo and 100,000-message phones scored −6.0 or higher. The filler scored −7.9 or lower.
- **Two passes:** the Assistant shows the vector results first and the checked ones when ready, so a CPU-only machine never waits on the check to see an answer.
- **Examiner control:** the check can be switched off per question, with a count of what it removed. If the calibration doesn't suit a case, nothing is hidden for good.
- **Result:**
  - On the 100,000-message phone, "messages about money" went from 25 confident matches (20 of them filler) to exactly the 5 real transfers.
  - On the demo phone, "Sure, send me a time." no longer answers "money".
  - Every end-to-end check still passes, on GPU and CPU.
- **Cost:** the check finishes shortly after the results are already on screen.
  - GPU: well under a second for most questions. About 2 s for a misspelled question, which is checked with both spellings.
  - CPU: 2–6 s.
  - The first question of a session also waits for the model to load (about 12 s, overlapping with typing).
- **Downloads:** the model comes with the Semantic Index plugin's approved download. The Assistant only uses it once it is on disk, and never downloads anything itself.

## 5. Bug fixes

- **Semantic Index could miss messages.** It could start while a Python message parser from the same run was still writing messages, and index only some of them. It now waits for the run's other plugin jobs first.
- **Date ranges.** "messages between april and march" returned January and February: the unordered "and" and the month typo were misread. Seven new tests cover this and the related cases above.
- **Result floods on large cases.** Z-score tiering alone passed the top few percent of everything. Rank caps and one-copy-per-text now keep answers readable.
- **Weak "strong" matches.** The best item of an unrelated group was always promoted, for example a house photo for "birthday". A strong match must now also clear an absolute score and stand out from its group.
- **Concurrent SQLite working copies.** Two copies of the same database at once could collide in one staging folder. Each copy now gets its own folder.
- **Model download consent** now reports what is actually missing, instead of a fixed size.

## 6. Tests and tooling

- **Frontend (`bun test`): 51 tests in 7 files, up from 15.**
  - Assistant question parsing, including the date ranges above.
  - Plugin selection defaults.
  - Case-folder-inside-evidence detection.
- **Rust (`cargo test --lib`): 64 tests, up from 41.** Both also have one manual benchmark, skipped by default.
  - Hashing.
  - Evidence SQLite copies: WAL rows visible and evidence unchanged.
  - Semantic scoring: tiering, rank caps, repeated texts, the precision veto, and 8-bit vectors scoring like float32.
  - Packed-vector decoding.
  - CUDA build selection from `nvidia-smi`, by driver and card (including older lab drivers and Pascal cards), and version pins.
  - Caption carry-over on re-scans.
- **Demo data (`test-data/demo`):**
  - `generate_demo_evidence.py` builds a fictional phone. The storyline is money laundering between John, Jane and Tony, and there are photos.
  - `--scale N` adds N filler messages, 40 filler contacts and N/100 photos for large-case testing (`--scale 100000`).
  - A JSON Chat Export test plugin parses the chat export.
- **End-to-end:** the release app was driven through its real UI on the demo, 10,000-message and 100,000-message phones, on GPU and CPU. Each run:
  - creates a case
  - adds the datasource
  - runs every plugin
  - visits every page
  - asks the Assistant's questions and checks their answers

## 7. Files

**New (backend)**

- `src-tauri/src/ai/`: shared local-AI runtime.
  - `model_worker.rs`: long-lived Python worker protocol, idle shutdown, stderr capture.
  - `packages.rs`: pinned packages, CUDA build selection, wheelhouse installs.
  - `captioning.rs` and `python/caption_worker.py`: BLIP worker.
  - `python/device.py`: GPU/CPU choice.
  - `mod.rs`: GPU status and Use GPU, warm-up, shutdown.
- `src-tauri/src/embeddings/`: semantic search.
  - `worker.rs` and `worker.py`: BGE-M3.
  - `scoring.rs`: tiers, caps, precision veto.
  - `store.rs`: vectors in the case database.
  - `corpus.rs`: in-memory case index.
  - `rerank.rs` and `rerank.py`: the precision check.
- `src-tauri/src/evidence_sqlite.rs`: evidence-safe SQLite working copies.
- `src-tauri/src/plugins/builtin/`: `hash_calculator`, `media_tagging` and `semantic_index` built-in plugins.

**New (frontend)**

- `src/features/assistant/`: the Assistant.
  - `AssistantPage.tsx`, `assistantEngine.ts`, `assistantQuery.ts`, `assistantDateRange.ts`, `assistantFormat.tsx`, `assistantExamples.ts`, `textSpans.ts`, `types.ts`.
  - `components/`: entry view, filter chips, match badges, message/media/call/contact lists, possible links, conversation dialog.
- `src/features/plugins/`: `AddPluginsDialog.tsx`, `PluginSelectionActions.tsx`, `AiAccelerationControl.tsx`, `useModelDownloadConsent.tsx`, `pluginSelection.ts`, `casePluginSettings.ts`, `useCasePluginSwitches.ts`.
- `src/features/files/fileHashRepository.ts`, `src/features/datasources/pathContainment.ts`, `src/app/reveal.ts`.

**Changed:** `index.html`, `tauri.conf.json` (CSP, window), `capabilities/default.json` (executable block), `lib.rs`, `plugins.rs`, and the Files, Media, Artifacts, Plugins and datasource wizard pages.

**Removed:** `plugins/builtin/media_tagging_inference.py`, the one-shot BLIP script, replaced by the long-lived worker.

## 8. Known limits and next steps

- **Code signing.** Release builds aren't signed. Labs that require signed software, and Windows Smart App Control, need a code-signing certificate.
- **Python plugins are code.** They run with the examiner's permissions, so install only trusted plugins. The built-in plugins ship with the app.
- **CPU speed.** CPU-only works everywhere but is slower than a GPU. Measured on a 12-thread CPU:
  - Indexing: 40 texts/s with native bfloat16, 21 without.
  - Captioning: about 1.4 photos/s.
  - Precision check: about 2.3 s per 40 candidates with native bfloat16, 4.3 s without.
  All three run in the background, and the answer appears first. Older CPUs without native bfloat16 get no speed-up from this branch's bfloat16 work.
- **Calibration data.** The precision veto was calibrated on synthetic test phones. It is deliberately conservative, removing only clear misses, and the examiner can switch it off per question to see what it removed. Labs with real casework can re-tune `CHECK_MIN_LOGIT` in `scoring.rs`.
- **Memory.** One case's index is held in memory at about 1.3 KB per indexed item (~0.25 GB at 200,000 items). Searching it is a full scan, about 0.1 s at 300,000 items. Much bigger cases would want an approximate-nearest-neighbour index.
- **Older GPUs untested on real hardware.** The CUDA 12.6 path for older drivers and Pascal/Maxwell cards was chosen from PyTorch's published builds. It couldn't be tried on such a machine. If a card can't run the build, its self-test fails and that model runs on the CPU, so the worst case is a wasted ~2.5 GB download.
- **Nothing is promoted.** The precision check only removes matches. Promoting matches that vector search ranked low would need a larger model.

## 9. Try it

```powershell
bun install
bun run tauri dev                    # or: bun run tauri build
bun test                             # frontend tests
cd src-tauri; cargo test --lib       # Rust tests
```

Generate the demo phone with `python test-data/demo/generate_demo_evidence.py` (add `--scale 100000` for a large case), add it as a datasource with every plugin ticked, then ask the Assistant "messages about money".
