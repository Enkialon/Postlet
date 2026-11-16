# Postlet

**Postlet** is a desktop mail client built with [Tauri 2](https://tauri.app/).  
It treats **contacts and conversations** as first-class citizens — not folders.

[简体中文](./README.zh-CN.md)

## Features

- **Conversation-first UI** — browse mail like chat threads with people and groups
- **Microsoft Graph (Outlook)** and **IMAP + SMTP** in one app
- Protocol routing by account (`receive_protocol` / `send_protocol`), not by brand name
- Local SQLite store; Graph / IMAP traffic stays in the Rust backend
- Soft-delete with optional “show deleted”, hide sessions without wiping mail
- Folder sync is opt-in after sign-in (pick which folders to sync)

## Tech stack

| Layer | Choice |
|---|---|
| Shell | Tauri 2 |
| UI | Vue 3 + TypeScript + Vite + Tailwind |
| Graph | `graph-rs-sdk` (Rust only) |
| IMAP / SMTP | Rust (`imap` / `lettre`) |
| DB | SQLite via SQLx |
| License | [GPL-3.0-only](./LICENSE) |

## Requirements

- Node.js 20+ and [pnpm](https://pnpm.io/)
- Rust stable (for Tauri)
- Platform deps for Tauri (see [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/))

## Develop

```bash
pnpm install
pnpm tauri dev
```

Frontend only:

```bash
pnpm dev
```

## Build

```bash
pnpm tauri build
```

Installers appear under `src-tauri/target/release/bundle/`.

## i18n

UI copy is Chinese-first. The Vite plugin rewrites static template strings to `$t(...)`.  
Extract new keys (merge-only, never overwrites existing translations):

```bash
pnpm i18n:extract
```

Locales live in `src/i18n/locales/`. Default language follows the OS; override in Settings.

## Releases

Pushing a `v*` tag runs [`.github/workflows/release.yml`](./.github/workflows/release.yml), which builds **Windows** and **macOS** (Apple Silicon + Intel) packages and attaches them to a GitHub Release.

## License

Copyright (C) 2026 本澈  

This program is free software: you can redistribute it and/or modify it under the terms of the **GNU General Public License as published by the Free Software Foundation, either version 3 of the License**, or (at your option) any later version — see [LICENSE](./LICENSE) (`GPL-3.0-only` in package metadata).
