# Parallax

A background voice-capture app that becomes a place to think in. Global hotkey from
anywhere, ramble, filed automatically. Over time entries find each other, and the app can
draw a line between two of them or ask about one.

**The interlocutor is your past self, not the AI.** A note from March isn't trying to be
agreeable to your November position. The AI retrieves it, puts it next to something
relevant, and asks.

This repository is currently a **UI mockup running on the real stack**: the Next.js
frontend is complete and exercised against a fixture backend. The Tauri shell is real; the
Rust command layer is not written yet.

## Running it

```bash
npm install
npm run dev
```

Opens at `http://localhost:3000`. That is where the mockup lives.

The desktop build works:

```bash
npm run tauri:dev     # dev server inside the Tauri shell
npm run tauri:build   # release binary + MSI and NSIS installers
```

If MSVC cannot link, the usual cause is a Windows SDK that is *registered but absent* —
the registry lists a version under `Windows Kits` while `Include` and `Lib` are missing
from disk, so adding the component is a no-op. Removing and re-adding
`Microsoft.VisualStudio.Component.Windows11SDK.26100` through the Build Tools installer
forces the payload back down.

The empty state offers a seeded sample corpus — 21 entries backdated across a year, which
is what the cross-time mechanics need to be visible at all. A reviewer opening an empty app
sees grey blobs and concludes it doesn't do much.

