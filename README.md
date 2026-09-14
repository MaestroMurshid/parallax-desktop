# Parallax

A background voice-capture app that becomes a place to think in. Global hotkey from
anywhere, ramble, filed automatically. Over time entries find each other, and the app can
draw a line between two of them or ask about one.

**The interlocutor is your past self, not the AI.** A note from March isn't trying to be
agreeable to your November position. The AI retrieves it, puts it next to something
relevant, and asks.

A Tauri 2 desktop app: a Next.js frontend over a Rust backend that records, transcribes
(whisper, on device), and reads notes with a local model (llama.cpp). Nothing leaves the
machine.

## Running it

```bash
npm install
npm run tauri:dev     # the app, with the Next.js dev server inside the Tauri shell
npm run tauri:build   # release binary + MSI and NSIS installers
```

The frontend only runs inside the desktop shell; opened in a plain browser it says so.
`scripts/fetch-llama.ps1` (Windows) or `scripts/fetch-llama.sh` (macOS, Linux) puts the
bundled llama-server in place before a build. Rust tests: `cargo test` in `src-tauri`.

If MSVC cannot link, the usual cause is a Windows SDK that is *registered but absent* —
the registry lists a version under `Windows Kits` while `Include` and `Lib` are missing
from disk, so adding the component is a no-op. Removing and re-adding
`Microsoft.VisualStudio.Component.Windows11SDK.26100` through the Build Tools installer
forces the payload back down.

The empty state offers a seeded sample corpus — 16 entries backdated across a year, which
is what the cross-time mechanics need to be visible at all. A reviewer opening an empty app
sees grey blobs and concludes it doesn't do much.

