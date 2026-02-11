# Tauri + SvelteKit + TypeScript

This template should help get you started developing with Tauri, SvelteKit and TypeScript in Vite.

## Shared queue (gist event log)

Set these environment variables before launching the app:

- `GEZELLIG_SHARED_QUEUE_GIST` (gist ID)
- `GEZELLIG_SHARED_QUEUE_FILE` (filename, e.g. `queue.ndjson`)
- `GEZELLIG_DJ_BOT=1` to run the bot instance that reads the queue and publishes audio

The queue file is NDJSON with append-only events:

```
{ "id": 1, "type": "queued", "url": "https://..." }
{ "id": 2, "type": "played", "ref": 1 }
{ "id": 3, "type": "failed", "ref": 1 }
```

## Recommended IDE Setup

[VS Code](https://code.visualstudio.com/) + [Svelte](https://marketplace.visualstudio.com/items?itemName=svelte.svelte-vscode) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer).
