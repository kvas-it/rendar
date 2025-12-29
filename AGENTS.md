# Repository Guidelines

## Project Structure & Module Organization
- Rust sources live in `src/`, with the CLI entry point in `src/main.rs`.
- Place reusable logic in modules under `src/` (e.g., `src/render/`, `src/preview/`, `src/config/`).
- Put integration tests in `tests/` and keep fixtures under `tests/fixtures/`.
- Store built-in theme assets under `assets/theme/`.

## Build, Test, and Development Commands
- `cargo run -- build --out dist` renders Markdown to HTML in `dist/`.
- `cargo run -- build --out dist --template template.html` uses a custom HTML template.
- `cargo run -- preview --open --port 3000` starts the dev server with live reload and opens a browser.
- `cargo test` runs unit/integration tests.
- `cargo fmt` formats Rust code; `cargo clippy` runs lint checks.

## Coding Style & Naming Conventions
- Follow Rust conventions: 4-space indentation, `snake_case` for functions/modules, `PascalCase` for types.
- Keep modules small and focused; avoid large `main.rs` by extracting helpers into `src/`.
- Use `clippy` defaults; prefer explicit error types in public APIs.

## Testing Guidelines
- Use Rust’s built-in test framework with `#[test]` functions.
- Name integration tests by feature (e.g., `tests/preview_server.rs`).
- Keep fixtures minimal and focused on real-world Markdown examples.

## Commit & Pull Request Guidelines
- Use concise, imperative commit messages (e.g., `Add user auth flow`, `Fix token refresh bug`).
- PRs should include a short summary, testing notes, and any relevant screenshots or logs.
- Link issues or tasks when applicable.

## Rendering & Preview Behavior
- Rendering mode converts a Markdown tree into HTML with a built-in theme and optional template override.
- Preview mode runs a local web server with file watching and live reload.
- Diagrams are rendered client-side with Mermaid.js; math uses KaTeX.

## Security & Configuration Tips
- Optional config lives in `rendar.toml` (default is zero-config from current directory).
- Keep secrets out of the repo; use `.env` only if future features require credentials.
