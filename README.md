# Tauri + SvelteKit + TypeScript

This template should help get you started developing with Tauri, SvelteKit and TypeScript in Vite.

## Shared queue (repo event log)

Set these environment variables before launching the app:

- `GEZELLIG_SHARED_QUEUE_REPO` (owner/name, e.g. `williammartin/gezellig-queue`)
- `GEZELLIG_SHARED_QUEUE_FILE` (path, e.g. `queue.ndjson`)
- `GEZELLIG_DJ_BOT=1` to run the bot instance that reads the queue and publishes audio

The queue file is NDJSON with append-only events:

```
{ "id": 1, "type": "queued", "url": "https://..." }
{ "id": 2, "type": "played", "ref": 1 }
{ "id": 3, "type": "failed", "ref": 1 }
{ "id": 4, "type": "playing", "ref": 1, "title": "Song Title", "url": "https://..." }
{ "id": 5, "type": "skip", "ref": 1 }
{ "id": 6, "type": "cleared" }
```

## Recommended IDE Setup

[VS Code](https://code.visualstudio.com/) + [Svelte](https://marketplace.visualstudio.com/items?itemName=svelte.svelte-vscode) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer).
