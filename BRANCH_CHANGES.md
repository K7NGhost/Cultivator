# What this branch adds

This document explains the branch `feature/fast-plugins-and-assistant-fixes`. It builds on `feature/semantic-assistant-bge-m3`, and together they turn Cultivator into a forensic tool with local AI analysis.

The goal is simple: an examiner loads a phone or folder of evidence, Cultivator processes it in the background, and the examiner can then ask plain questions like "messages about money between John and Jane in March" and get answers they can check.

Three rules shaped every decision:

- **Everything runs on the examiner's machine.** Evidence is never uploaded. The only time Cultivator goes online is a one-time download of the AI models, and only after the examiner agrees to it.
- **It must work on ordinary lab hardware.** A computer with no graphics card still does everything, just more slowly. A graphics card, even an older one, makes it faster.
- **Evidence must never change.** Opening, searching and analyzing evidence leaves the original files exactly as they were.

## How the pieces fit together

When a datasource (a folder or phone extraction) is added to a case, Cultivator runs plugins over it:

1. **Message parsers** read chats, calls and contacts.
2. **Hash Calculator** fingerprints every file.
3. **Media Tagging (AI)** describes every photo in words.
4. **Semantic Index (AI)** turns every message and photo description into a searchable "meaning" index.

The **Assistant** then uses all of that to answer questions.

## File hashing

The Hash Calculator plugin records the MD5, SHA-1 and SHA-256 fingerprint of every file in one pass, for chain-of-custody verification.

- It is on by default.
- The hashes appear in the Files view and the file preview.
- **Copy Hashes** in a file's right-click menu copies them.

### Hash sets

Hash sets are lists of known file hashes that every case is checked against (**Analysis → Hash sets…**).

- **Known-bad lists** flag files, for example Project VIC / CAID exports or a lab's own lists.
- **Known-good lists**, such as NSRL, mark operating-system and program files that are usually safe to set aside.
- **Any common format imports:** plain lists, CSV (including NSRL's), hashdeep output and JSON exports. MD5, SHA-1 and SHA-256 are all recognised.
- **Stored once, for all cases.** Lists live on the examiner's computer, not inside a case.
- **Matches** appear under **Files → Hash Set Hits**, one entry per list.

### Extension mismatch

**Files → File Views → Extension Mismatch** lists files whose content contradicts their name, such as a `.jpg` that is really a zip, a classic way to hide data in plain sight. Cultivator reads each file's signature, accepts related extensions (a `.docx` is a zip inside), and ignores generic ones like `.dat`. The demo phone includes a "photo" that is really a zip of the ledger.

## Tags and notes

Examiners can tag files and photos (Bookmark, Follow Up, Notable, Project VIC categories, or any custom tag) and add a note.

- Tagging works from the Files page and from a photo's details in the Media page.
- The tags are stored with the case and listed under **Files → Tags**.

## Timeline

The Timeline page (Analysis → Timeline) puts every dated message, call, photo and video in the case on one list:

- **Activity over time:** a chart by day, or by month for long cases. Clicking a bar zooms into that period.
- **Activity by hour of day:** shows when the phone is used ("pattern of life").
- **Filters:** by type, date range and text (a person or words).
- **Click through:** clicking an event opens it: messages and calls in Artifacts, photos and videos in Media.

## Map

The Map page (Analysis → Map) plots photos by their GPS coordinates, plus any location artifacts from plugins.

- **Built-in world map:** the map is bundled into the app, so viewing it never sends evidence locations to an online map service.
- **Navigation:** scroll to zoom, drag to move, and **Fit** frames every point.
- **Point details:** clicking a point shows the photo, its exact coordinates (with a copy button) and a link to open it in Media.

## Photo captions and tags

The Media Tagging plugin looks at each photo with a local AI model (BLIP) and writes a short caption, such as "a red car driving down a road", plus keyword tags. That makes photos searchable by what they show. The Media page displays the captions and tags and has a search box.

Captioning is built for large collections:

- The model loads once and stays loaded while photos are processed in batches.
- Large photos are shrunk before analysis, and photo rotation is respected.
- HEIC photos are supported when the HEIC reader is installed.
- When a datasource is processed again, photos that haven't changed keep their existing captions, so only new or changed photos cost time.

## Search by meaning

The Semantic Index plugin uses a second local model (BGE-M3) to capture the meaning of every message and photo caption, so a search matches ideas rather than exact words. Searching for "money" also finds "Sent 9,500 to the warehouse account", even though the word "money" never appears.

- Each distinct text is processed only once, even if the same message appears hundreds of times.
- Progress is saved in small batches, so a cancelled run picks up where it stopped.
- The plugin waits for the message parsers in the same run to finish, so it never indexes a half-imported phone.

## The Assistant

The Assistant (Analysis → Assistant) answers plain-language questions about the open case.

### Understanding the question

A rule-based parser reads people, dates and the kind of result wanted (messages, calls, contacts, photos, videos) out of the question. It shows its interpretation as a row of chips, so the examiner can see exactly what was searched and remove or change any part of it.

Dates are read the way people write them:

- **Typos** in month names are fixed ("apirl" becomes April). Names of people in the case are never mistaken for months.
- **Either order works:** "between april and march" means March to April.
- **No year needed:** "before april", "after june", "since may" and "until july" all work.
- **Exact days:** single days and day-level ranges are supported.

### Finding matches

Words in the question are matched both exactly and by meaning.

- Spelling mistakes are corrected ("tranactiosn" finds transactions), and both spellings are searched in case the correction is wrong.
- Confident matches form the answer. Weaker ones are collected in a folded **Possible links** section, so they are available without crowding the answer.
- On very large cases, only the best matches are kept, and a message repeated many times counts once. The answer stays readable instead of listing thousands of loose hits.
- Filler words in the question ("show me ppl trying to…", "someone", "being") are ignored, so they don't dilute the search.

### Investigator themes

Questions about behaviour name an idea that the messages themselves never state. On the 100,000-message test phone, "New burner number soon." ranked around 31,000th for "discreet".

For these questions, Cultivator uses built-in themes: secrecy, destroying evidence, threats, fraud and drugs. When the question calls one up:

- The typical wording of that behaviour ("Delete these texts", "Use the burner phone", "Keep your mouth shut or else") is searched alongside the question.
- The precision check judges matches against a description of the behaviour, not just the one word.
- A mixed question such as "hiding money" also searches its topic ("money") on its own.
- The answer shows which theme was used ("Meaning search (theme: secrecy)").

Measured on the test phone:

- **"discreet":** 6 of the real secrecy messages now appear, up from 0.
- **"threats":** found the threat.
- **"fraud":** found 4 of 5 fraud messages with no filler.

### The precision check

Meaning search is fast but approximate. On a large phone, short everyday lines such as "The keys is on the counter." can land near the top of almost any search. On a 100,000-message test phone, 20 of the top 25 matches for "money" were filler like that.

To fix this, a third local model (Qwen3-Reranker) reads the question together with each of the best matches and judges whether they are really related. It understands indirect connections: a transfer counts as being about money. For the same "money" search it removed all 20 filler lines and kept the 5 real transfers.

How it behaves for the examiner:

- **Results appear immediately.** The check then runs and quietly removes the weak matches: under a second on a graphics card, a few seconds on a processor.
- **Checked matches carry a small tick.**
- **The examiner stays in control.** A **Precision check (removed N)** chip shows how many matches were removed. Removing that chip shows everything again, and it can be switched back on.
- **It only removes, never adds.** It is deliberately cautious and removes only clear misses.

### Following a result

Every result leads back to the evidence:

- A message opens its whole conversation with the hit highlighted, with links to the artifact and the original file.
- Photos and videos open in the Media page.
- Calls and contacts open in the Artifacts page.

## Running plugins

- **Install once, use everywhere.** Installed plugins stay installed. Every one is ticked when a datasource is added, with **Select all** and **Clear** buttons, and each case remembers the selection.
- **Per-case switches.** A plugin that isn't wanted in one case can be switched off in the **On** column of Plugins → Installed Plugins, without uninstalling it for other cases.
- **Add plugins later.** Plugins → Datasource Jobs → **Add** runs extra plugins on a datasource that was already processed.
- **One clear download prompt.** Before an AI model is downloaded for the first time, a single dialog lists what will be downloaded and how big it is: about 1.5 GB for captioning, and 3.5 GB for search plus the precision check.
- **Graphics card status.** The Plugins page shows whether AI runs on the graphics card or the processor. A **Use GPU** button switches an existing installation to the graphics card.
- **A safety check.** The datasource wizard refuses a datasource that contains the case folder itself, which would otherwise feed the case's own files back in as evidence.

## Evidence safety and lab readiness

- **Evidence is never modified.** Phone databases (SQLite) change on disk when they're opened, even when opened read-only. Cultivator therefore works on a private copy in a temporary folder and deletes those copies when it starts and exits. Recent unsaved data that phones keep beside the database (the "WAL" file) stays visible. An automated test confirms the evidence files' fingerprints don't change.
- **Nothing in the evidence can run.** Programs, scripts, installers and shortcuts found in evidence (37 risky file types, such as `.exe`, `.ps1`, `.bat`, `.lnk` and `.msi`) can't be launched from Cultivator. The examiner is pointed to the text/hex preview or "Reveal in folder" instead.
- **No outside code.** The app loads no scripts from anywhere else. A strict Content Security Policy blocks any that evidence content might try to inject. A development script that could have been hijacked by another program on the machine was removed.
- **Offline once set up.** After a model is downloaded, it runs with network access turned off, and telemetry is disabled. The Assistant never downloads anything itself.
- **Air-gapped labs.** A lab with no internet can copy the models folder from a connected machine, and point Cultivator at a folder of pre-downloaded Python packages.
- **Reproducible results.** The exact versions of every AI model and package are fixed. Each AI plugin's log records which model version ran and on which device, for the audit trail.
- **Safe model files.** The precision-check model loads only from the safetensors format, which can't carry hidden code. An interrupted download is detected and retried rather than half-used.
- **Open licenses.** All three models allow this use: BLIP (BSD-3-Clause), BGE-M3 (MIT) and Qwen3-Reranker-0.6B (Apache-2.0).

## Works on ordinary lab hardware

Most forensic workstations have plenty of memory, storage and cooling, but not the newest graphics card or processor. The branch is built for that.

- **No graphics card needed.** Every feature runs on the processor alone. On processors that support a faster number format (bfloat16, found on many recent AMD chips), search indexing and the precision check automatically run about twice as fast with the same results. Photo captioning stays in the standard format, because the faster one changed the captions.
- **Graphics cards when available, including older ones.** Cultivator reads the NVIDIA driver and card and installs the matching version of its AI engine (PyTorch):
  - Current drivers and cards get the newest build.
  - Older certified drivers (R528 and later), common on managed lab machines, get an older compatible build.
  - Older cards such as the GTX 10-series and Quadro P-series also get that build, and run in full precision, where they are much faster.
  - If a card can't run a model, or doesn't have enough free memory, that model quietly runs on the processor instead.
- **Uses memory well.**
  - Search models stay loaded for 30 minutes after the last question, so there is no reload delay after a short break.
  - The Assistant's in-memory index is stored compactly, about 1.3 KB per message.
- **Handles long runs.** Heavy work runs in the background. It can be cancelled and resumed, and the examiner can keep using the app while it runs.

## Built for large cases

The branch was tested on a generated phone with 100,000 messages and 1,012 photos:

- All plugins finished in about two minutes on a laptop graphics card.
- Assistant questions were answered in about half a second once the case was loaded.

What makes that possible:

- Models stay loaded instead of restarting for each batch.
- Each distinct text is processed once.
- Large batches are sent to the graphics card, and data moves in compact binary form.
- The case's index stays in memory between questions.
- Answers are capped to the best matches.
- The precision check only reads the top matches, so its cost doesn't grow with the size of the case.

Everything also works on a processor alone, only more slowly; the demo phone takes under 30 seconds.

## Fixes along the way

- **Semantic Index could miss messages.** It could start while the message parser was still importing messages, and index only some of them.
- **Date ranges.** "messages between april and march" returned January and February.
- **Large cases.** Searches on large cases returned thousands of loose matches.
- **Weak best matches.** The best item of a completely unrelated group was shown as a confident match, for example a house photo for "birthday".
- **Working copies.** Two copies of the same database made at the same moment could collide.
- **Download prompt.** The prompt showed a fixed size instead of what was actually missing.

## Tests and demo data

- **Frontend: 58 automated tests.** They cover:
  - question parsing (including the date cases above)
  - plugin selection
  - the case-folder safety check
  - reading GPS coordinates
  - the built-in world map
  - timeline activity charts
- **Rust: 69 automated tests.** They cover:
  - hashing and reading hash-set files
  - extension mismatch
  - evidence-safe database copies
  - search scoring, the precision check and investigator themes
  - compact vectors
  - choosing the right AI engine for a driver and card
  - keeping captions for unchanged photos
- **Demo phone generator** (`test-data/demo`):
  - It builds a fictional phone with a money-laundering storyline between John, Jane and Tony, with photos, calls and contacts.
  - The story covers secrecy, destroying evidence, a threat, police, travel and fraud.
  - It includes a "photo" that is really a zip of the ledger.
  - `--scale 100000` adds 100,000 messages for large-case testing.
  - A JSON chat-export test plugin reads the demo messages.
- **End-to-end runs.** The finished app was driven through its real screens on the demo, 10,000-message and 100,000-message phones, on a graphics card and on the processor alone. Each run creates a case, adds the datasource, runs every plugin, visits every page, asks the Assistant's questions, checks the answers, and flips the precision-check switch.

## Known limits

- **Code signing.** The installer isn't signed yet. Labs that require signed software, and Windows Smart App Control, need a code-signing certificate.
- **Python plugins.** Python plugins are programs, and they run with the examiner's permissions. Install only trusted ones. The built-in plugins ship with the app.
- **Processor-only speed.** Machines without a graphics card are noticeably slower: indexing is roughly 20–40 messages per second and captioning about 1–2 photos per second. Both run in the background.
- **Precision-check calibration.** The check was calibrated on generated test phones. It is cautious and can be switched off per question. Labs with real casework can re-tune its cutoff (`CHECK_MIN_LOGIT` in `scoring.rs`).
- **Older graphics cards.** The older-card and older-driver path couldn't be tried on real hardware. If it doesn't work on a machine, that machine simply uses the processor.
- **Very large cases.** A case of several hundred thousand distinct messages uses a few hundred MB for the in-memory index and scans it in about a tenth of a second. Much bigger cases would benefit from a dedicated search index.

## Trying it

```powershell
bun install
bun run tauri dev                    # or: bun run tauri build
bun test                             # frontend tests
cd src-tauri; cargo test --lib       # Rust tests
```

Generate the demo phone with `python test-data/demo/generate_demo_evidence.py` (add `--scale 100000` for a large case). Add it as a datasource with every plugin ticked, then ask the Assistant "messages about money".
