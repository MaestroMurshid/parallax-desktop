# Parallax — backend architecture

## Context

The repo is a complete Next.js frontend running against an in-memory fixture backend.
`lib/bridge/tauri.ts` is written in full and names **33 Tauri commands that do not exist**.
`src-tauri/src/lib.rs` is 154 lines and implements **two** of them (`show_capture`,
`hide_capture`) plus the tray, two windows, close-to-tray, and a hardcoded
`Ctrl+Shift+Space` global shortcut. `.env` sets `NEXT_PUBLIC_BRIDGE=mock`, so the Tauri
path has never executed.

The goal is a working local-first backend: hotkey → record → transcribe → classify →
embed → place → persist → ask, with cross-time edge proposal on a background pass.

Stack is fixed (handoff §9.0, non-negotiable): **Tauri + Rust**, **Next.js static export**,
**transcribe.cpp** via Rust SDK / C bindings, **llama-server** with local GGUF management.
Handoff §9 additionally fixes: `cpal` capture, **fastembed-rs** embeddings, **SQLite**,
**brute-force cosine** — *"You will never need a vector database."*

**The contract is already written.** `lib/bridge/index.ts` is the interface,
`lib/bridge/tauri.ts` is the command-name mapping, `lib/types.ts` is the wire shape, and
`lib/bridge/mock.ts` is a 789-line reference implementation with the exact semantics. This
plan is mostly about *executing a written spec*, not designing one.

**The governing constraint is one week**, for an evaluation, by a backend Java developer
new to Rust and the AI toolchain. That shapes everything: see **Constraints** and
**The week** below, which are the sections to read if you read only two. The Architecture
section is the design; the Milestones section is what happens if the work continues past
the week.

---

## Step 0a — correct `handoff.md`, then bring it into the repo

Four Sonnet agents checked every falsifiable claim in the doc against the tree. It is
substantially out of date, and since source comments and commit bodies cite it by section
number, the corrections should land before it becomes the reference for Rust work. Rewrite
and commit as `docs/handoff.md`.

**Corrections to apply, by section:**

| § | Claim | Reality |
|---|---|---|
| 1 | "auto-connection" runs on new entries | Only the `answers` edge is auto-created; all other edges are fixture data or manual |
| 1.2 | Action items "detected during the same pass that classifies the entry" | `createEntry` always sets `actionItems: []`; they exist only in the seed corpus |
| 1.2 | Inertness shares the probe-suppression machinery | No shared path — action items are inert because nothing calls a probe on them |
| 3.1 | Nine modes A–I | Five exist as probes (B, C, D, E, F). **A is unimplemented**, G is a `Relation`, **H is entirely unbuilt**, I is the `note` role |
| 3.2 | "never F" automatically | Reversed deliberately — Feynman fires automatically on `evidence` (`classification.ts:249-253` explains why) |
| 3.2 | "felt / inert / under ~30s" | Now `register === 'live'`, `role === 'note'`, `!hasOwnSpan`, `durationMs < 30_000` |
| 3.2 | Invoked path allows "any mode" | `evidence`/`note` can only ever reach feynman |
| 3.3 | Münchhausen's full gating list | **None of it exists.** Gated identically to steelman |
| 3.6 | Rendered types "claim / rant / felt, plus inert" | `position` / `evidence` / `note`, with `register` as a treatment on top |
| 3.6 | User-type fields `name/match/prompt/tier/render` | Also `id`, `mark`, `autoApproved`; `name`→`label`, `render`→`role` |
| 3.6 | Rule 1 via `autoApproved` | Dead field. Real gate is `tier` ∈ `AUTO_FIRING` |
| 4 | Discard key works | **Not registered as a global shortcut**; unreachable while the panel is up |
| 4 | "one question appears in serif underneath" the panel | Panel never shows a question — `transcribing` → `saved` (1400ms) → closes |
| 4 | Long recordings make the panel release rather than hold | `RELEASE_AFTER_MS` only gates whether a question is generated; panel behaviour is identical |
| 5.1 | "Manual links don't move nodes" | True for placed nodes, but `linkWeight: 1.25` outranks `strongThreshold: 0.55`, so a link is the *strongest* signal for a new entry |
| 5.2 | Size = duration (blob radius) | Duration sets **title font size** (10.5–19px, sqrt off 60s). There is no blob |
| 5.2 | Fingerprint = 7–9 bars from real amplitude | `signatureBars(id, width)` draws **5–10 bars from an id hash**; `entry.fingerprint` is never read by anything that draws |
| 5.2 | Type in the edge treatment (crisp/irregular/soft) | `edgeTreatmentFor` is transitively dead. Role shows as a glyph + font |
| 5.3 | The markers table | **Four of five render nothing** — rings, resolved ring, question dot and dashed stroke are all dead code |
| 5.3 | `#D85A30` is "the only colour in the entire app" | Dark theme only; light uses `#1D4ED8` |
| 5.4 | Seven relations incl. `echoes` | **Eight**, `echoes` removed, `answers` and `related` added; six are model-emittable |
| 5.4 | Labels at the midpoint | Hover-only, above a zoom threshold, and slid off-midpoint to clear titles |
| 5.5 | "Never load transcripts into JS" / query per viewport | `loadCorpus` loads the entire corpus incl. transcripts; `visibleBounds` is unused |
| 6.1 | Analysis collapsed by default | `analysisOpen: true`, and reset to `true` on every open/close |
| 6.2 | Answers thicken the parent's rings | Reversed (§17) — but only half-done: `Canvas.tsx:21-22` still hides them |
| 6.3 | "Never overwrite. Stack." | Stacks across child entries; repeated resolve/reopen on one entry **overwrites** `resolutionText` |
| 7 | Schema sketch | Omits 8 real `Entry` fields; the `vectors` table does not exist in any form |
| 7.1 | `topic_vec` / `move_vec` | No embedding field exists. `moveCluster` is populated on every fixture entry and **read by no code** |
| 7.2 | Background pass | Entirely absent |
| 8 | `edge labels #6E7C7F` | Now `#7D8B8E`; `#6E7C7F` is the dimmed/isolated colour |
| 8 | "~15px serif question" | Live question is `clamp(16px, 2.4cqw, 19px)`; the 15px token is onboarding-only |
| 8 | "Nothing animates except live audio" | Three panels slide in at 190ms |
| 9.1 | `next.config.js` | `next.config.**mjs**`, plus `devIndicators: false` |
| 9.3 | GGUF 1.5B / 3B / 7B | Catalogue ships Qwen3 1.7B / 4B / 8B (1.05 / 2.4 / 4.7 GB) |
| 9.4 | Per-entry `localOnly` set at capture | Global default only; displayed but not editable |
| 13 | "20–30 entries over roughly a year" | **16 entries over ~23 months** |
| 13 | Corpus properties | Missing three of six: **0 resolved**, **0 threads**, **0 `same move` edges** |
| 14 | Search / empty state / sparse state "not drawn" | All three built |
| 15 | Manual edge drawing "still absent" | Built, with three entry points (§16 is the correct one) |
| 15 | `src-tauri/` does not compile | **It does** — `parallax.exe` + MSI + NSIS, 5 Sep |
| 15 | Seeded corpus is 20 degrowth entries, 5 isolated | 16 entries on reasoning/AI/cognitive bias, 3 isolated. Degrowth corpora are the retired `.bak`s |
| 15 | Theme resets to `system` | Resets to `light` (`ui.ts:52`) |
| 15 | Onboarding and empty state share a sentence | No longer true |
| 15 | Search dims titles but not edges | Search now filters edges to the matched subgraph and draws them heavier |
| 17 | Answer edge uses `extends` | Uses `answers` — the "eighth word" was already added |
| 17 | Corpus is 14 position / 4 evidence / 2 note, 8 live | **8 / 6 / 2, and 0 live** |
| 17 | `digital-degrowth-book` carries an attributed span | Entry no longer exists; **every** entry's `attributedQuotes` is empty |
| 17 | Onboarding beat order | models → **setup** → types → links (setup is beat 2, not 4) |
| 17 | "Camera does not fit the corpus on first load" | Built — `fitToContent()` runs once (`Canvas.tsx:70-169`) |

Still-open items that **verified as genuinely still open**: source-material marker; the
question prompt; within-entry reversal; challenges-the-answer vs challenges-the-question;
`register` unmeasured; mode H unbuilt; no type-correction path; accepted connections have no
surface; `200 days apart` hardcoded; import has no version check; legend reserves 172px.

## Step 0b — commit the pending work

Uncommitted on `main`, unrelated to the backend, should land first:

- `components/onboarding/Backdrop.tsx` (new) — deterministic blurred field behind setup.
- `Onboarding.module.css` — backdrop layer; `.advanced` re-anchored to `.rows`.
- `Onboarding.tsx` — renders `<Backdrop/>`; drops the residency picker from first-run.
- `lib/shell.ts` — **real bug fix**: webview-scoped listener replaces global `listen`, so
  one hotkey press no longer starts two recordings.

One open call: the diff relabels transcription `transcribe.cpp {name}` → `Whisper {name}
{quant}`. Handoff §9.0 names transcribe.cpp as the engine; the model is Whisper. Keeping
the diff as-is (name the weights, not the engine) unless told otherwise.

**Also: bring `handoff.md` into the repo** (`docs/handoff.md`). Source comments across
`lib/` and every commit body cite it by section number — §3.2, §5.1, §7.1, §9.4 — and it
currently lives only in `~/Downloads`. It is the authoritative design record and the Rust
layer will cite it constantly. `AGENTS.md` holds only the Next.js auto-generated block and
is the natural place to record Rust conventions once settled.

---

## Architecture

### Process shape

```
Tauri process (Rust)
├─ webviews: main (canvas) + panel (capture)     — exists
├─ tray + global shortcut                        — exists, hotkey hardcoded
├─ SQLite (rusqlite, bundled) + r2d2 pool
├─ cpal input stream → ring buffer → WAV
├─ transcribe.cpp   (in-process, load/unload per use, §9.3)
├─ fastembed-rs     (in-process, load/unload per use)
├─ enrichment queue (tokio worker)
└─ llama-server     (managed CHILD PROCESS, localhost, warm|cold per Settings.residency)
```

llama-server is the only out-of-process piece. Everything else is in the Tauri process.

### Module layout — `src-tauri/src/`

```
lib.rs            builder, plugins, tray, windows          (exists; extend)
state.rs          AppState: pool, providers, vector index, queue handles
error.rs          Error enum → serde, for invoke returns
model/            wire types mirroring lib/types.ts, serde camelCase
db/               migrations + one module per table + search
audio/            capture.rs (cpal), wav.rs, undo buffer
stt/              trait Transcriber, transcribe_cpp.rs
llm/              trait LlmProvider, llama_server.rs, remote.rs, prompts/, grammar.rs
embed/            fastembed wrapper, similarity.rs (brute-force cosine)
scene/            PORTS of the TS pure modules — see below
enrich/           job queue, classify, question, edges (background pass)
models/           catalogue, resumable downloader, sysinfo profile
commands/         thin #[tauri::command] wrappers, grouped as in bridge/index.ts
sample/           seed corpus asset + loader
```

`commands/` stays thin — a wrapper per bridge method, all logic in the modules below it.

### Ported logic — the parity requirement

Five frontend modules are pure and **must be reimplemented in Rust with identical output**,
because the frontend already derives appearance from what the backend stores:

| TS source | Rust port | Why it must match exactly |
|---|---|---|
| `lib/scene/placement.ts` | `scene/placement.rs` | positions are frozen at insert (§5.1); a divergent port renders the sample corpus differently |
| `lib/scene/relax.ts` | `scene/relax.rs` | `tidy` and the sample-corpus settle pass |
| `lib/scene/vector.ts` | `scene/vector.rs` | `hash32`/`rng` seed the deterministic fallbacks |
| `lib/scene/markers.ts` | `scene/markers.rs` | `detectUnfinished` — 12 hedge regexes, **regex not a model** (§5.3) |
| `lib/scene/classification.ts` | `scene/gates.rs` | `automaticProbes` / `mayProbeOnRequest` / `hasOwnSpan`. `mock.ts:440` re-enforces these server-side rather than trusting the UI; Rust must too |

These are mechanical ports against an exact reference — **dispatch Sonnet subagents, one
module each**. Gate each with a golden-file test: dump `fixtures/load.ts` output (entries
with x/y, unfinished, filtered questions) to JSON, assert the Rust port reproduces it.

### Data layer

SQLite, one file in the app data dir. Tables map 1:1 onto `lib/types.ts`:

```
entries(id PK, audio_path, transcript, created_at, x, y, parent_entry_id,
        answers_question_id, role, register, type_id, resolved, resolution_text,
        title, summary, duration_ms, fingerprint, unfinished, local_only, is_sample)
spans(id, entry_id, start, end, attributed)
action_items(id, entry_id, span_start, span_end, text, done)
edges(id, entry_a, entry_b, relation, question, status, created_at)
questions(id, entry_id, text, span_start, span_end, answered, dismissed,
          provider_name, created_at)
vectors(entry_id, topic_vec BLOB, move_vec BLOB, dim)
types(id, label, match_text, prompt, tier, role, mark_kind, mark_char, auto_approved)
tokens(entry_id, start, end, confidence)   -- §9.5, excluded from similarity when low
settings(key, value)
jobs(id, entry_id, kind, state, attempts, error, created_at)
```

Three deliberate choices:

- **`parent_entry_id`, not `parent_edge`.** The wire field `parentEdge` holds a parent
  *entry* id, not an edge id. Fix the name in the schema and rename at the serde boundary
  (`#[serde(rename = "parentEdge")]`) so the trap stops at the wall.
- **No FTS5.** The contract is substring-by-default with `"quoted"` meaning whole-word
  (`bridge/index.ts:62-68`). FTS5 tokenises differently and cannot do substring. A Rust
  scan reproduces `mock.ts:299-329` exactly, including snippet offsets. Personal-corpus
  scale; revisit only if measured slow.
- **Vectors as f32 BLOBs**, loaded into an in-memory `RwLock<HashMap>` at startup, scanned
  brute-force. Per §9: a few thousand 384-dim vectors is ~15MB, sub-millisecond.

### llama-server owns every LLM call

**llama-server with local GGUF management is the default and first-class path (§9.4) — it
is what gets demoed, and it must be a working implementation, not a stub.** Every
generation in the app goes through it:

| Call | When | Shape |
|---|---|---|
| **Enrichment / classify** | synchronously in `stop_recording` | one GBNF-constrained JSON response: `title`, `role`, `register`, `typeId`, `summary`, `spans[]` + `attributed`, `actionItems[]`, `movePhrase` |
| **Question generation** | `get_question`, `ask_question`, `run_probe` | one call per probe (steelman / boundary / disconfirming / munchhausen / feynman) under §3.4 stance rules, anchored to a non-attributed span |
| **Relation naming** | background edge pass | one cheap call per candidate pair — *what* is the relation; emit nothing unless it names one of the six |

Plus everything around it, which §9.4 calls *"a first-class feature, not plumbing"*:
GGUF download, checksum verification, selection and hot-swap; process spawn on a free
localhost port with a health check; warm/cold residency per `Settings.residency`; and kill
on exit. `Settings.providerName` already defaults to `'llama-server'` and is rendered on
every question in the UI — the user always knows who answered.

`RemoteProvider` sits behind the same `LlmProvider` trait as a *bonus* (§9.4: *"it must
never be the reason the product looks good"*), and an entry marked `localOnly` never
reaches it — including as context for a different entry's question.

### Embeddings — a separate runtime, not a substitute

Embeddings are **not** an LLM call. They turn text into a 384-dim float array for cosine
similarity; nothing is generated. §9 lists both in the same Rust stack sentence, as
distinct components: *"transcribe.cpp bindings, `fastembed-rs` (ONNX) embeddings, SQLite,
brute-force cosine."*

**fastembed-rs, MiniLM, 384-dim, loaded per use.** Not llama-server's `/embedding`, for
four reasons that are all in §9:

- §9.0: *"Audio and embeddings are local unconditionally."* Embeddings sit **outside** the
  `LlmProvider` abstraction — only the reasoning LLM is swappable.
- §9.3 budgets them as a separate line item (`MiniLM ~90MB, load per use`) from the GGUF.
- **Residency.** `Settings.residency` may be `cold`. Placement happens at insert and is
  frozen forever (§5.1). Sourcing vectors from llama-server would stall every cold capture
  10s+ before the entry could be given a position.
- §9 already sized it: *"a few thousand 384-dim vectors is ~15MB and sub-millisecond to
  scan. You will never need a vector database."* `lib/scene/vector.ts` says the same.

**Neither vector is embedded from the raw transcript** (§7.1):

| | embedded from | scope |
|---|---|---|
| `topic_vec` | the **generated summary** — *"a six-minute ramble embeds mushy; digressions dilute it"* | all spans |
| `move_vec` | an **LLM-extracted phrase naming what the entry does** — e.g. *"denies X is exempt from a general pattern"* | **own spans only** (§7.3) |

`move_vec` is the differentiator and came out of real data: three of the user's own notes —
microbes and divine hierarchy, AI-generated art, their own writing process — shared no
vocabulary but made the same move (refusing human exceptionalism). Topic cosine would never
have paired them.

Three consequences for the implementation:

1. **Own spans only.** A note quoting three people would otherwise be connected on *their*
   thinking; on the real corpus examined during design, over half a mature note was block
   quotes. `Span.attributed` already exists on the wire type — this is its second consumer,
   alongside the probe anchor.
2. **Two thresholds, not one.** §5.4: run move at a *lower* bar than topic — *"topical hits
   are common and cheap; move matches are rare and meaningful."*
3. **`move_vec` cannot exist at insert** — it needs the LLM, so it lands on the enrichment
   queue. Placement uses `topic_vec` alone. Acceptable because move_vec only drives edges,
   and the background pass excludes the last two weeks anyway (§7.2).

Expect layout to shift once this is real: §15 flags that similarity is currently faked from
`storedType` + an id hash, so *"a `contradicts` edge does not pull at all"*.

### The two pipelines

**Synchronous — `stop_recording`, must return a complete `Entry`:**

```
stop stream → WAV → transcribe.cpp → classify (ONE structured LLM call)
            → embed summary → place → persist → return Entry
```

One grammar-constrained (GBNF) call returns `{title, role, register, typeId, summary,
spans[], actionItems[], movePhrase}` — not eight calls. Mock budgets 1800ms here.

**Degraded path.** §7.1 embeds `topic_vec` from the *generated summary*. If llama-server
is cold or down, embed the transcript instead, derive a heuristic title (`mock.ts:777`
`deriveTitle`), default `role`/`register`, and enqueue re-enrichment. Position is assigned
now and **stays frozen** (§5.1) — a later summary refreshes `topic_vec` for retrieval but
never moves the node. This is the §9.4 fallback made concrete.

**Asynchronous — the enrichment queue:**

- re-enrichment for entries that took the degraded path
- `move_vec`: embed the LLM-extracted move phrase over **own spans only** (§7.3)
- questions for recordings over `RELEASE_AFTER_MS` (150s), which skip the sync question
- the background edge pass

**Background edge pass (§7.2):** excludes the last ~2 weeks so every proposal is
cross-time — *"That one line is most of the product."* kNN on both vectors at different
thresholds (move at a **lower** bar than topic, §5.4), one cheap LLM call per candidate
pair asking *what* the relation is, emit only if it names one of the six `MODEL_RELATIONS`.
If the best it can say is "related", **draw nothing**.

### Tauri config changes

The shell config is currently sized for a mockup and needs four changes:

- **`Cargo.toml`** — `serde`/`serde_json` are declared but unused; `tauri` has only the
  `tray-icon` feature. Needs `protocol-asset` (audio playback), plus rusqlite, tokio, cpal,
  fastembed, sysinfo, reqwest, keyring. Also revisit `opt-level = "s"` — size-optimised is
  the wrong default once inference is in-process — and `panic = "abort"`, which rules out
  `catch_unwind` around audio callbacks and model inference.
- **`tauri.conf.json`** — `"csp": null` is the loosest setting in the repo and becomes a
  real exposure once model-authored text renders and HTTP downloads run. Also: `bundle` has
  no `resources` and no `externalBin`, so nothing is currently set up to ship the seed
  corpus, a model file, or a llama-server binary.
- **`capabilities/default.json`** — custom commands need no grant, so the 33 commands land
  without touching it. New grants *are* needed for the asset protocol (playback), and for
  `dialog`/`fs` when export/import goes native in M5.
- **`.env`** — delete `NEXT_PUBLIC_BRIDGE=mock`. It is tracked deliberately and its own
  comment says to remove the line when the Rust commands land.

### Relation vocabulary — two sets, enforced server-side

`lib/types.ts` has eight `Relation` values but `MODEL_RELATIONS` has six. `answers` is
generated by the system when `parentEdge` is set; `related` is **manual-only**. Commit
`5ad3348` is explicit: §5.4 stops the app drawing a line it cannot name, but that is not the
same as a person who knows two notes belong together and cannot yet say why. The edge
proposal path must reject anything outside the six.

---

## Constraints this plan is built around

**One week. Author is a backend Java developer; Rust and the AI toolchain are new. System
design is the strength.** The evaluator wants a working product, product thinking, and
evidence of learning the AI tooling well.

Three consequences:

- **Reduce Rust surface, not product surface.** The mechanical work — the pure-module
  ports, the 33 near-identical command wrappers, the serde types — goes to Sonnet subagents
  with golden-file tests as the gate. Hand-write the parts where judgement lives: schema,
  the audio path, the LLM integration, the degraded path.
- **Spike the risky integrations on day one, as throwaway code**, before anything is built
  around them. The two that can sink the week are `transcribe.cpp` (C++ toolchain; `cmake`
  is not on PATH, and the README documents a Windows SDK that registers without installing)
  and llama-server process management. Both are *small* in code volume and *high* in
  toolchain risk — the inverse of where time normally goes.
- **Idiomatic Rust is not the deliverable.** Clone liberally, `Arc<Mutex<_>>` for shared
  state, no lifetime gymnastics. Fighting the borrow checker on a deadline is how this
  stalls.

### What demonstrates "learned the AI tooling well"

Not volume of ML code. Four specific things, all cheap:

1. **GBNF grammar-constrained decoding** so the classify call returns valid JSON every
   time instead of being parsed hopefully. This is the llama.cpp technique that separates
   someone who read the docs from someone who ran the thing.
2. **The §13 sweep** — run two or three GGUF sizes, read the questions, report the finding.
   §9.4 is explicit: if no local size clears the bar, *say so as a finding* rather than
   quietly demoing on a hosted API. A reported negative result reads as judgement.
3. **Not gating on model confidence** (§17): seven 3–9B instruct models tested in 2026 all
   scored invalid on confidence validity, ceilinged at 91.7% regardless of accuracy. Tuning
   sensitivity by prompt instead is the correct call and a non-obvious one.
4. **The capacity math** behind rejecting a vector database.

---

## The week

**Day 0 — spike, throw away.** Get `transcribe.cpp` to transcribe one WAV from a Rust
binary, and get llama-server to spawn, answer one prompt, and die cleanly. Nothing else.
If transcribe.cpp will not build, that is known on day one and the fallback (upstream
binary as a Tauri sidecar via `externalBin`, documented as a deviation) still leaves six
days.

**Days 1–2 — storage and the command layer.** Schema + migrations, serde wire types, then
the read/CRUD commands. Largest code volume, lowest risk, closest to the author's existing
skills. Delete `NEXT_PUBLIC_BRIDGE=mock` at the end of day 2 — the app then runs on real
data with no ML at all, which is the first demo-able state. → **most of this to Sonnets**,
once the first two commands establish the pattern.

**Day 3 — audio.** cpal capture → 16kHz mono WAV → `capture://amplitude` at 30Hz →
transcribe.cpp. Hotkey read from settings and re-registered on `set_settings`. At the end
of day 3 the hotkey produces a real transcribed entry.

**Day 4 — llama-server and the one enrichment call.** `LlmProvider` trait,
`LlamaServerProvider`, spawn/health/residency. One GBNF-constrained call returning
`title`, `role`, `register`, `typeId`, `summary`, `spans[]`, `actionItems[]`, `movePhrase`.

**Day 5 — embeddings, placement, questions.** fastembed-rs; the ported `placement.rs`
using real vectors; question generation behind the Rust-side gates. This is the day the
product becomes itself.

**Day 6 — connections, on demand rather than scheduled.** See below.

**Day 7 — README, demo path, buffer.** Assume day 7 is buffer; something will overrun.

### Frontend work the backend exposes (half a day, high demo value)

Verified by grep: six canvas functions are defined and **called by nothing**, so four of
the five markers in §5.3 currently render as nothing —

| Dead function | What is not drawn |
|---|---|
| `ringSpecs` (`lib/scene/blob.ts:98`) | return-count rings; the solid resolved ring |
| `questionDotOffset` (`blob.ts:123`) | the unanswered-question dot — *"the only colour in the entire app"* |
| `fingerprintBars` (`blob.ts:61`) | the amplitude waveform |
| `dashedCircle` (`renderer.ts:361`) | the `unfinished` dashed stroke |
| `markersFor` (`markers.ts:53`) → `edgeTreatmentFor` | transitively dead |
| `visibleBounds` (`lod.ts`) | viewport culling — unused scaffolding |

The one that directly wastes backend work:

```ts
bars: signatureBars(entry.id, sigWidth)   // LabelOverlay.tsx:66
```

`signatureBars` is keyed on the **entry id**, not `entry.fingerprint`. Nothing that draws
ever reads `fingerprint`. So cpal will produce a real amplitude envelope, the backend will
store it faithfully, and the canvas will keep drawing a hash of the id. Passing the real
fingerprint through is close to a one-line change and it is what makes *"every recording
looks different"* (§5.2) true.

Budget half a day to wire these four back up. The geometry is already written; only the
call sites are missing. Without it the backend computes `fingerprint`, `unfinished`,
questions and return counts that the UI silently discards.

### Bugs found during verification (fix while in the area)

Confirmed by grep, not inferred:

1. **The discard hotkey cannot fire.** `discardHotkey` appears nowhere in `src-tauri/src/`
   — `lib.rs:123` registers only `Ctrl+Shift+Space`. Discard is a DOM `keydown` at
   `app/page.tsx:138`, but the panel is created `"focus": false` and `lib.rs:53-55`
   deliberately never focuses it, so while the panel is up the keypress goes to whatever
   app holds focus. §4 promises a discard key; there isn't a working one. Fixing it is part
   of M1's "register `discardHotkey` too" anyway.
2. **`autoApproved` is dead and silently overwritten.** Read by no gating function, and
   `classification.ts:107` forces it `true` on every custom type regardless of what
   `TypeEditor.tsx:80` set. The real gate is `tier` via `AUTO_FIRING` (`:91`, read at
   `:266`). Either delete the field or wire it — leaving a safety-shaped flag that does
   nothing is the worst of the three options.
3. **The §17 answer reversal is half-done.** `mock.ts:395-408` creates an answer as a peer
   entry with an `answers` edge and comments that it "lands on the canvas", but
   `Canvas.tsx:21-22` still filters out every entry with `parentEdge !== null` under the
   old comment. Answers are peer-shaped in data and invisible on the map.
4. **`evidence` is `tier: 'silent'` yet fires Feynman automatically** (`classification.ts:86`
   vs `:265-267`). `TypeEditor.tsx:34-42` special-cases the gloss so the UI doesn't say
   "never asks on its own" — meaning the tier model is already known to be leaky and is
   patched at the label rather than the gate.

### Fix the seeded corpus (cheap, JSON only)

`fixtures/corpus.json` is the demo surface and currently fails three of §13's six required
properties: **0 entries with `resolved: true`** (no resolution stack, so rings show
nothing), **0 entries with `parentEdge`** (no threads), and **0 edges with `same move`** —
the differentiator relation. `moveCluster` is populated on every entry and read by no code.
16 entries over ~23 months, not the 20–30 over a year the doc claims.

This is a JSON edit, not Rust, and it is what a reviewer actually sees on open.

### The one scope change that matters

**`list_proposed_edges(entryId)` computes on demand and caches, instead of a scheduled
background pass.** The UI already calls it when an entry opens. Same kNN over both vectors,
same relation-naming LLM call, same six-relation constraint — just triggered rather than
scheduled. This gets the cross-time mechanic *working and visible in the demo* without
building a scheduler, and the seeded corpus supplies the time depth it needs.

Write up the scheduled pass (§7.2, including the two-week exclusion) as designed-not-built,
with the reason: the exclusion window and the queue are trivial to add on top of a
retrieval path that already works, and the retrieval path is the part carrying risk.

### Cut, and written up instead

| Cut | Substitute for the week | Why it is safe to cut |
|---|---|---|
| Model download UI + catalogue | Ship with models pre-fetched to a known path | `download_model` "gates nothing" per the bridge doc; capture works without it |
| `RemoteProvider` | Trait defined, one implementation | §9.4 calls remote a bonus that "must never be the reason the product looks good" |
| Scheduled background pass | On-demand proposals (above) | Same code path, minus a scheduler |
| Type / theme persistence, batch commands, native dialogs | — | Known gaps, already listed in §15 "accepted for the mockup" |
| `undo_discard`, per-word confidence | — | Never implemented in the mock either; no reference behaviour exists |
| `relax.ts` port | `tidy` stays frontend-only, as today | Already works in TS; nothing requires it server-side |

---

## Milestones (post-week, if it continues)

### M0 — Ground work

- Step 0a/0b (correct the handoff, commit the pending work); `cmake` on PATH. The MSVC
  chain itself already works — see Risks.
- Cargo deps, module skeleton, `Error`, `AppState`.
- Schema + migrations (`user_version`).
- Wire types with serde. **`ModelState` is the one non-obvious shape** — internally-tagged
  on `kind`, needs `#[serde(tag = "kind", rename_all = "kebab-case")]`.
- The five ports + golden-file parity tests. → **Sonnet subagents.**

### M1 — The spine: capture → store → read back *(the agreed first deliverable)*

- All read/CRUD commands against SQLite: `list_entries`, `get_entry`, `list_children`,
  `list_edges`, `list_action_items`, `set_action_item_done`, `move_entry`, `delete_entry`,
  `search_entries`, `create_entry`, `resolve_entry`, `reopen_entry`, `create_manual_edge`,
  `accept_edge`, `dismiss_edge`, `list_proposed_edges`, `dismiss_question`, `get_settings`,
  `set_settings`, `import_corpus`, `load_sample_corpus`, `clear_sample_corpus`.
- Cascade semantics copied from `mock.ts`: `delete_entry` orphans children rather than
  deleting them; `clear_sample_corpus` sweeps entries orphaned by the removal; `import`
  in `replace` mode wipes, `merge` skips known ids and drops edges with a missing endpoint.
- `cpal` capture → 16kHz mono WAV; `capture://amplitude` at ~30Hz (payload is a bare
  `f32`; `Equalizer.tsx` writes straight to DOM refs, so 30Hz not 300).
- transcribe.cpp with the model from `Settings.transcriptionModel`; store word confidences.
- Discard + **60s undo** — keep the WAV, delete after `UNDO_WINDOW_MS`. The mock never
  implemented this (`undoDiscard` always returns null), so there is no reference behaviour.
- Hotkey **read from settings** and re-registered on `set_settings`; register
  `discardHotkey` too. Both are hardcoded/absent in `lib.rs:123` today.
- Real audio playback: asset protocol + a capability scoped to the audio dir; swap the
  simulated clock in `lib/store/playback.ts` for an `<audio>` element.
- Delete `NEXT_PUBLIC_BRIDGE=mock` from `.env`.

Entries land with a heuristic title and default classification. No LLM yet.

### M2 — Models and providers

`sysinfo` profile · catalogue · resumable download with checksum · `model://progress` ·
failure states · llama-server spawn on a free localhost port with health check and
warm/cold residency · `LlmProvider` trait with `LlamaServerProvider` (default, first-class
per §9.4) and `RemoteProvider`. Onboarding stops being fiction.

### M3 — Enrichment

The structured classify call + GBNF grammar · fastembed-rs both vectors · real placement ·
question generation under §3.4 stance rules with the Rust-side gates · anchors exclude
`attributed` spans (§17) · job queue with retry.

### M4 — Cross-time

Background pass with the 2-week exclusion · **contradiction retrieval (mode G) first**, per
§13 — strongest and least dependent on prompt quality · dismissals persisted as training
signal (`bridge/index.ts:108`).

### M5 — Contract gaps and desktop parity

| Gap | Fix |
|---|---|
| Questions accumulate but only one survives a restart — `loadCorpus` stores one per entry | add `list_questions()` |
| `loadCorpus` is N+1 (one `get_question` per entry) | the same batch command |
| Custom types are zustand-only (`lib/store/types.ts:7`) and vanish on reload | `list_types` / `upsert_type` / `delete_type` |
| Theme not persisted (handoff §15, accepted for the mockup) | add to `Settings` |
| `relayout()` fires one `move_entry` per node | batch `move_entries` |
| Export/import use browser Blob + `<a download>` | Tauri dialogs + fs |
| Import has no version check — a pre-facet export loads `role: undefined` (§17) | version the envelope |
| `localOnly` displayed but not editable per entry | per-entry toggle; enforce that a local-only entry never reaches `RemoteProvider`, including as context for another entry |
| API key for `RemoteProvider` | OS keychain, never SQLite or settings JSON |

---

## Verification

- **Parity tests** — golden files from `fixtures/load.ts` vs the Rust ports (x/y,
  `unfinished`, gate-filtered questions). This is the test that matters most: the frontend
  derives all appearance from stored data, so a divergent port is silently wrong.
- **Command-surface test** — `cargo test` asserting `generate_handler!` covers every name
  in `lib/bridge/tauri.ts`. Cheap, catches drift.
- **End-to-end, real app** — `npm run tauri:dev` with `.env` mock removed. Hotkey from
  another window → panel appears bottom-right → speak → stop → entry lands on canvas →
  reopen the app → it is still there. Then `npm run tauri:build` and repeat on the installed
  binary, since the tray/hotkey path differs.
- **Offline check** — pull the network. The local path must work end to end (§9.4: *"Left
  until last, the local path ends up a stub — and that's the version that fails an offline
  check."*).
- `npm run typecheck` and `npm run build` stay clean throughout; `next build` must keep
  producing a static export.

---

## Risks

1. **`transcribe.cpp` needs resolving to an actual crate/repo before M1.** The name is
   fixed by the brief; which Rust SDK or C binding it means is not yet established here.
   Blocking for M1 — resolve it first.
2. **Windows build chain — narrower than the docs claim.** `src-tauri/` *does* compile:
   `target/release/parallax.exe` (3.8MB, 5 Sep) plus MSI and NSIS bundles exist, so §15's
   "does not compile / `kernel32.lib` missing" and the README's SDK warning are both stale
   as live problems. The remaining risk is specific: `Cargo.lock` has **zero** entries for
   whisper, cpal, fastembed or rusqlite, and `cmake` is not on PATH — so no C++-backed
   crate has ever been built here. That is what day 0 is for.
3. **Gate logic lives on both sides.** `EntryView` explains *why* a question was suppressed
   (`silenceReason()`); if the Rust gate disagrees with `classification.ts`, the UI narrates
   a reason that did not apply. Parity tests cover this.
4. **Placement quality will change** once embeddings are real — handoff §15 flags that some
   of what currently looks like a layout weakness is the mocked similarity.
5. **Question quality at local GGUF sizes is unmeasured.** §13 says run the sweep *first*.
   If no local size clears the bar, §9.4 is explicit: report it as a finding rather than
   quietly demoing on a remote API.

---

## Day 0 — RESULT: both integrations work

Run 8 Sep on the target machine (Ryzen 5 5600H, RTX 3050 Laptop 4GB, 13.9GB RAM).
**Both risky integrations are green.** The week's plan holds.

### transcribe.cpp

Builds and transcribes. `transcribe-cpp` 0.2.3 compiled in 40s; total build 210MB.

```
load 82 ms | transcribe 315 ms for 11.0s audio (34.9x realtime)
```

- Model: `handy-computer/whisper-tiny-gguf` → `whisper-tiny-Q5_K_M.gguf`, **43MB**.
  Repos are `whisper-{tiny,base,small}-gguf`, matching `mock.ts`'s catalogue.
- **82ms load settles §9.3's question**: load-per-use is right, do not hold whisper resident.
- 35x realtime means transcription is effectively free — a 2-minute note lands in ~3.5s.
- Confirms `16 kHz mono f32 [-1,1]`, which is what cpal must be configured to emit.

### Toolchain — three fixes, all required

1. **CMake is not installed and Build Tools 2026 does not bundle it.** `winget install
   Kitware.CMake` (got 4.4.3).
2. **The Visual Studio generator is broken with Build Tools 2026.** CMake 4.4.3 emits a
   project MSBuild then cannot find (`MSB1009`) during its compiler probe. Fix:
   `winget install Ninja-build.Ninja` and set `CMAKE_GENERATOR=Ninja`.
3. **CMake caches its generator**, so switching generators against a used build dir fails
   regardless. Wipe `%LOCALAPPDATA%\tcs` (the short-path symlink transcribe-cpp-sys creates
   to dodge MAX_PATH) before retrying.

Build must run under `vcvars64.bat` so Ninja can find `cl.exe`. The spike lives at
`D:\parallax-spike` — a short path on the drive with room.

### llama-server + the §13 model sweep

Prebuilt binaries: `ggml-org/llama.cpp` release **b10867**, `bin-win-cpu-x64` and
`bin-win-cuda-12.4-x64` (+ cudart). Server answers `/health` in ~1s and honours
OpenAI-style `response_format: json_schema`.

Same prompt, same transcript (`free-will-own-reasoning`), against the fixture's authored values:

| | fixture | Qwen3-1.7B | Qwen3-4B |
|---|---|---|---|
| `role` | position | position ✓ | position ✓ |
| `register` | neutral | **live ✗** | neutral ✓ |
| `summary` | accurate | **misstates the claim ✗** | accurate ✓ |
| `movePhrase` | — | **copied the prompt's example ✗** | own phrasing ✓ |
| `title` | "our own reasoning" | "free will" ✗ | "Free Will and Reasoning" ✗ |
| latency (CPU) | — | 3.4s | 9.0s |
| latency (GPU) | — | — | **1.8s** |

**Findings that change the plan:**

- **Constrained decoding is total.** Every response was valid against the schema, correct
  enums, parseable first time. Shape is guaranteed by the grammar — but *correctness is not*.
  That distinction is the thing to say out loud in the writeup.
- **1.7B is not good enough.** Valid JSON, unreliable content. 4B is the floor.
- **4B on the 3050 hits 1.8s**, inside §4's ~2s budget, so the synchronous post-recording
  question is viable. On CPU it is 9s and is not. **GPU offload is not optional.**
- **VRAM is the ceiling: 3114 / 4096 MiB at 4B Q4 with `-c 4096`.** 8B is impossible on this
  machine, and a larger context risks OOM. §9.4's "small warm model + large cold model"
  split is therefore *not available here* — there is room for exactly one resident model.
- **Warm residency costs VRAM, not system RAM** — better than §9.3 assumed for a 14GB machine.
- **`register` failed at 1.7B and is the first live evidence for §17's warning** that the
  facet may not be callable. Worth reporting as a finding either way.
- **Do not put concrete examples in the system prompt.** 1.7B copied the `movePhrase`
  example verbatim. The move phrase is the differentiator, so this is the prompt most
  worth iterating on.
- **Neither model produced a §5.2 title.** Both reach for the topic ("Free Will and
  Reasoning") instead of the speaker's own phrasing ("our own reasoning"). Title generation
  needs its own prompt, probably its own call, and is a known-open item rather than a solved one.
- **`movePhrase` at 4B restates content rather than abstracting the move** — subject-bound,
  so it would not cluster microbes with AI-art. The differentiator is not yet working and is
  the highest-value prompt to iterate on.

### Disk

`cargo clean` on `src-tauri/target` freed **7.1 GiB** (E: 3.4 → 8.7GB free). Built installers
preserved to the scratchpad first. Keep model and build artefacts on D:; E: is too tight.
