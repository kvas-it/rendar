# rendar

Rendar renders a directory tree of Markdown files into static HTML and provides a live preview server while you work. It aims for zero-config usage, with optional `rendar.toml` settings for customization.

## Quick Start
```bash
cargo run -- build --out dist
cargo run -- preview --open
```

## Commands
- `build --out <dir> [--input <dir>] [--template <file>] [--config <file>] [--exclude <pattern>]`
- `check [--input <dir>] [--config <file>] [--exclude <pattern>]`
- `preview [--input <dir>] [--template <file>] [--config <file>] [--start-on <path>] [--open] [--port <port>] [--exclude <pattern>]`

## Config (Optional)
Create `rendar.toml` in the working directory:
```toml
input = "docs"
template = "theme.html"
exclude = ["**/AGENTS.md", "**/CLAUDE.md"]

[preview]
port = 4000
open = true
```

CLI flags override config values when provided.

`exclude` patterns use glob syntax, like `**/AGENTS.md` for any depth or `private/**` to skip a folder. Pass `--exclude` multiple times to add patterns.

## Templates
The built-in template ships with a minimal theme, Mermaid.js, and KaTeX. Custom templates can use these placeholders:
- `{{title}}` page title (defaults to the Markdown filename)
- `{{content}}` rendered Markdown HTML
- `{{nav}}` sidebar navigation HTML
- `{{breadcrumbs}}` breadcrumbs HTML
- `{{style}}` built‑in CSS (empty for custom templates)
- `{{extra_head}}` and `{{extra_body}}` internal hooks for preview reload

## Markdown Features
- GitHub-flavored Markdown extras (tables, task lists, strikethrough, footnotes).
- Mermaid diagrams via fenced code blocks:
- ` ```mermaid`
- `graph TD;`
- `  A-->B;`
- ` ````
- Math via KaTeX with `\(...\)` or `$$...$$`.

## Linking Behavior
- Links to `.md` files are rewritten to `.html` during render.
- `README.md` acts as the default page for a folder when no `index.md` exists.
- Local Markdown links that point to missing files emit a warning at render time.
- `check` prints warnings only and exits with status 1 when any are found.

## Navigation
- Each page includes a left sidebar with sibling pages and subfolders that have an index or README.
- Breadcrumbs are shown at the top and skip folders without an index/README.
- Page titles are taken from the first Markdown heading when present.

## Preview Start Page
- Use `--start-on` to open a specific Markdown file or directory when previewing.
- If `--input` is omitted and the start page is outside the current directory, rendar auto-detects the root by walking upward through folders with an index/README.
