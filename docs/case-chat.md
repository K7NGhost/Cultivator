# Local case chat

Open **Chat** from the workspace sidebar. The conversation view includes saved case history, a multiline composer, streaming replies, stop, copy, an evidence-category scope and inspectable source cards. Conversations and source snapshots are stored alongside the case index in `chat.sqlite`, not in browser storage. The history list shows the most recent 100 conversations; opening one loads its most recent 200 messages. Deleting a conversation requires confirmation and does not modify original evidence.

## Setup

1. Download **Qwen3-8B (Q4_K_M)** in Settings → Models. The official GGUF is approximately 5.03 GB, pinned to revision `7c41481f57cb95916b40956ab2f0b139b296d974` and checked with SHA-256 by the existing model manager. Interrupted downloads can be resumed. Chat never downloads model weights automatically.
2. Index the active case. For semantic retrieval, also download BGE Large and enable Semantic indexing in Settings. Chat can fall back to keyword retrieval with an explicit notice when semantic retrieval is unavailable.
3. Ask a case-specific question. Click a citation or source card to inspect its original record identity, path, retrieval methods and readable excerpt.

First generation provisions a dedicated uv/Python 3.12 environment. On Windows x64 it uses the official prebuilt llama-cpp-python 0.3.19 CPU wheel, avoiding a compiler requirement. The initial runtime uses four CPU threads and an 8192-token context, leaving GPU memory available for BGE. This is a portable CPU implementation, not a claim of CUDA acceleration. Provisioning dependencies needs connectivity; inference runs locally without a model service or evidence uploads. The model process is reused between turns and released when the active case changes.

## Retrieval architecture

```mermaid
flowchart TD
  Q[Investigator question] --> R[Rust orchestrator: active case and evidence scope]
  R --> T[Tantivy: planned keyword query]
  R --> B[BGE Large: query embedding]
  B --> L[LanceDB: semantic passages]
  R --> S[SQLite: exact quoted result IDs]
  T --> F[Reciprocal rank fusion and deduplication]
  L --> F
  S --> F
  F --> V[SQLite provenance checks and bounded exact row lookups]
  V --> C[At most eight evidence excerpts with source IDs]
  C --> M[Local Qwen3-8B]
  M --> A[Streamed draft, citation validation, saved answer and sources]
```

Planning is deterministic query preparation, not arbitrary model-generated SQL. SQLite verifies artifact IDs against the latest completed parser jobs and confirms their source paths. Small source rows are read exactly; large results retain their matching indexed excerpts. Reranking uses reciprocal rank fusion, not a separately downloaded neural reranker. Evidence scope currently means the active case and selected category (all, artifacts, file content or metadata), rather than unrestricted temporal/spatial SQL analysis.

Artifacts retain result IDs, original row indices and paths. LanceDB hits expose their original group identity for exact verification. Stale artifact references are excluded. Answers retain their retrieval warnings and source snapshots so they can be reviewed later even if the index changes.

The generator treats evidence and history as untrusted data. ChatML control tokens in that data are escaped, thinking is disabled, context is token-counted, and older history is removed before source excerpts if the budget is exceeded. The worker reports which evidence IDs actually fit the prompt. Rust withholds completed answers with missing or unknown citation IDs. Citation validation proves only that references identify supplied evidence; it does not prove that a claim is entailed. Investigators must verify generated claims against original records.

The model cannot run commands, generate/exercise SQL, open files or browse. Chat history is context, not evidence. No-evidence retrieval returns an explicit limitation without generation; absence from a retrieved sample is not proof of absence from the case. Cancellation during retrieval or generation stops the request; the shared vector worker can resume its checkpointed reconciliation after restarting.

## Validation boundary

Tests cover rank fusion, citation whitelisting, exact SQLite verification, stale artifact rejection, per-case history persistence, prompt role boundaries, token budgets and safe UI rendering. Browser fixtures cover streaming, source inspection, stop, new/history conversations, deletion confirmation and case switching. The actual Windows inference runtime was imported successfully. Full Qwen generation and retrieval-answer quality require manually downloading the model and evaluating representative case questions; mock browser replies are not model-quality validation.

Sources: [Official Qwen3-8B GGUF](https://huggingface.co/Qwen/Qwen3-8B-GGUF), [llama-cpp-python](https://llama-cpp-python.readthedocs.io/en/stable/).
