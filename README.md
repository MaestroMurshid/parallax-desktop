# Parallax

A voice-first place to think. Press a hotkey from anywhere, say what's on your mind, and
it's transcribed and filed. Over time notes find each other: Parallax puts an old note
next to a new one and asks how your thinking moved.

**It doesn't do your thinking for you.** You do the thinking; the AI assists. It files what you said,
finds what you said before, and asks you a question.

Test your reasoning, check your understanding, or make up your own note types: however you
want the thoughts in this commonplace book to be challenged or interpreted.

Everything runs on your machine. Transcription, reasoning and embeddings are local GGUF
models; the only network activity is the one-time model download.

## What it does

- **Capture** — a global hotkey opens a small floating panel that records without taking focus. Typed notes work too.
- **File** — each note is classified as a *position*, *evidence* or a plain *note*, given
  topics, and titled. You can define your own note types.
- **Question** — arguments get a question that pushes on them: a hidden assumption, a
  counterexample, a steelman and so on. The model picks the tactic. Your answer
  becomes a note of its own, linked to the question.
- **Connect** — new notes are compared with older ones, and a connection names how two notes
  relate (extends, contradicts, same move, returns to…) with the exact words from each.
  Every connection can be dismissed.
- **Ask** — search your notes, or ask a question of them ("how did my view on free will
  change?"). Answers quote what you said and when. Ask never writes to your notes.
- **Verbatim** — the transcript is the note. The only edit is correcting what the
  transcription misheard.
- **List or canvas** — a reverse-chronological list is the way in; the canvas shows the
  graph of connections.
- **Portable** — export as a zip of MDX files (one per note, with topics and connections in
  the frontmatter, optionally with audio) or as Markdown, and upload to merge or replace.

## Models

| Job | Model | Runs on |
|---|---|---|
| Transcription | Whisper tiny / base / small | CPU |
| Reasoning | Qwen3 4B (Q4_K_M, ~2.5 GB), or 8B on machines with the memory | GPU, CPU fallback |
| Embeddings | BGE small (default), MiniLM L6, Nomic embed | CPU |

Onboarding recommends models for your machine and downloads them. You can also point
reasoning or transcription at your own GGUF file in Settings. Reasoning runs on a bundled
llama.cpp (Vulkan on Windows and Linux, Metal on macOS).

Qwen3 4B is the smallest model that classifies notes reliably. Local models do a lot of
heavy lifting here, and results vary: connections and questions are useful but not
guaranteed.

## Building

Prerequisites: Node 22, stable Rust, and the
[Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your platform.

```bash
npm install
```

Fetch the pinned llama.cpp runtime into `src-tauri/binaries/llama`:

```bash
./scripts/fetch-llama.sh      # macOS, Linux
```

```powershell
./scripts/fetch-llama.ps1     # Windows
```

Then:

```bash
npm run tauri:dev     # the app, with the Next.js dev server inside the Tauri shell
npm run tauri:build   # release build and installers
```

The frontend only runs inside the desktop shell; in a plain browser it says so.

On first launch with nothing recorded, the empty state offers a **sample corpus**: 16 notes
spread across a year, which is what connections and questions across time need in order
to show up at all.

### Tests

```bash
npm run typecheck
npm run build                 # the Rust crate embeds the exported frontend from out/
cd src-tauri && cargo test
```

## Stack

Tauri  · Rust · Next.js (static export) · React · Zustand · PixiJS · SQLite ·
whisper.cpp · llama.cpp

## License

[AGPL-3.0](LICENSE)
