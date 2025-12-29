# rendar

Rendar renders a directory tree of Markdown files into static HTML and provides a live preview server while you work. It aims for zero-config usage, with optional `rendar.toml` settings for customization.

## Quick Start
```bash
cargo run -- build --out dist
cargo run -- preview --open
```

## Commands
- `build --out <dir> [--input <dir>] [--template <file>] [--config <file>]`
- `preview [--input <dir>] [--template <file>] [--config <file>] [--open] [--port <port>]`

## Config (Optional)
Create `rendar.toml` in the working directory:
```toml
input = "docs"
template = "theme.html"

[preview]
port = 4000
open = true
```

CLI flags override config values when provided.

## Templates
The built-in template ships with a minimal theme, Mermaid.js, and KaTeX. Custom templates can use these placeholders:
- `{{title}}` page title (defaults to the Markdown filename)
- `{{content}}` rendered Markdown HTML
- `{{style}}` built‑in CSS (empty for custom templates)
- `{{extra_head}}` and `{{extra_body}}` internal hooks for preview reload

## Markdown Features
- GitHub-flavored Markdown extras (tables, task lists, strikethrough, footnotes).
- Mermaid diagrams via fenced code blocks:
- ` ```mermaid`
- `graph TD;`
- `  A-->B;`
- ` ````
- Math via KaTeX with `$...$` or `$$...$$`.

## Linking Behavior
- Links to `.md` files are rewritten to `.html` during render.
- `README.md` acts as the default page for a folder when no `index.md` exists.
- Local Markdown links that point to missing files emit a warning at render time.
