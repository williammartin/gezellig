# AGENTS.md — Gezellig Development Guidelines

## Project Overview
Gezellig is a Tandem-like virtual office desktop app built with Tauri v2 (Rust backend + Svelte frontend). It provides team presence awareness, audio rooms via LiveKit, and a Spotify DJ feature via librespot.

## Development Methodology

### Outside-In TDD
Follow an outside-in TDD workflow for feature development:

1. **Receive user story or task**
2. **Write a failing Playwright acceptance test** that captures the expected behavior
3. **Run the test** — confirm it fails (RED)
4. **TDD the backend logic** using `cargo test`:
   - Write a failing unit test (RED)
   - Write the minimal code to make it pass (GREEN)
   - Refactor while keeping tests green (REFACTOR)
5. **Build the UI** (Svelte components) to connect backend to frontend
6. **Run the acceptance test** — confirm it passes (GREEN)
7. **Refactor** the full stack while keeping all tests green

### When to Write Tests
- **Backend/business logic**: Always TDD (red-green-refactor). No production Rust code without a failing test first.
- **UI components**: Covered by Playwright acceptance tests. Individual component unit tests are not required.
- **Integration points** (LiveKit, librespot): Write integration tests where feasible; use mocks/stubs for external services in unit tests.

### Test Organization
```
tests/
  acceptance/          # Playwright acceptance tests
    *.spec.ts
src-tauri/
  src/
    *.rs               # Unit tests inline with #[cfg(test)] mod tests {}
  tests/
    *.rs               # Integration tests
```

## User Story Workflow

When given a user story like:
> "As a team member, I want to see who is online so I know who I can talk to"

1. Parse the story into acceptance criteria
2. Write Playwright test(s) for each criterion
3. Implement following the outside-in TDD loop above
4. Verify all acceptance tests pass

Direct technical tasks (e.g., "add librespot dependency") should still follow TDD where applicable.

## Code Style

### Rust
- Follow idiomatic Rust conventions
- Use `clippy` with default lints — fix all warnings
- Format with `rustfmt`
- Prefer `Result<T, E>` over `unwrap()`/`expect()` in production code
- Use `anyhow::Result` for application-level errors
- Use `thiserror` for library-level error types
- Keep functions small and focused
- Only add comments for non-obvious "why", not "what"

### Svelte
- Use Svelte 5 runes syntax where available
- Keep components small and single-purpose
- Format with Prettier
- Lint with ESLint

### General
- Prefer composition over inheritance
- Keep dependencies minimal
- Study Zed's patterns (especially `crates/call/` and `crates/livekit_api/`) for LiveKit integration inspiration, but don't copy-paste — adapt to our simpler use case

## Architecture Boundaries

### Tauri Commands (Rust → Frontend)
All backend functionality is exposed via Tauri commands. The frontend never directly accesses LiveKit, librespot, or system audio — it goes through Tauri commands.

### Event Flow (Backend → Frontend)
Use Tauri events for real-time updates (participant joined/left, DJ changed, now playing, etc.). The frontend subscribes to these events.

### Key Crates
- `livekit` — LiveKit Rust SDK for room management, audio publishing/subscribing
- `librespot` — Spotify Connect device, PCM audio access
- `tauri` — Desktop shell, commands, events
- `anyhow` / `thiserror` — Error handling
- `tokio` — Async runtime

## Testing Tools
- **Rust**: `cargo test` (unit + integration)
- **Acceptance**: Playwright (via `@playwright/test`)
- **Agentic verification**: Use Playwright MCP browser tools to take snapshots, interact with UI, and verify visual state during development

## Agentic Development Notes
- The AI agent can use Playwright MCP to visually verify UI changes
- When iterating on UI, take a browser snapshot after changes to confirm correctness
- For backend changes, always run `cargo test` to verify
- Prefer small, incremental changes — commit after each passing acceptance test
