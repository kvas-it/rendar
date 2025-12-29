use anyhow::{Context, Result};
use pulldown_cmark::{html, CowStr, Event, Options, Parser, Tag, TagEnd};
use std::path::{Path, PathBuf};

pub struct RenderedPage {
    pub html: String,
    pub warnings: Vec<String>,
}

pub fn first_heading_title(markdown: &str) -> Option<String> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_GFM);
    options.insert(Options::ENABLE_SMART_PUNCTUATION);
    let parser = Parser::new_ext(markdown, options);

    let mut in_heading = false;
    let mut buffer = String::new();

    for event in parser {
        match event {
            Event::Start(Tag::Heading { .. }) => {
                in_heading = true;
                buffer.clear();
            }
            Event::End(TagEnd::Heading(_)) if in_heading => {
                let title = buffer.trim().to_string();
                if !title.is_empty() {
                    return Some(title);
                }
                in_heading = false;
            }
            Event::Text(text) if in_heading => buffer.push_str(text.as_ref()),
            Event::Code(text) if in_heading => buffer.push_str(text.as_ref()),
            Event::SoftBreak | Event::HardBreak if in_heading => buffer.push(' '),
            _ => {}
        }
    }

    None
}

pub fn render_markdown_file(
    path: &Path,
    input_root: &Path,
    index_dirs: &std::collections::HashSet<PathBuf>,
) -> Result<RenderedPage> {
    let markdown = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read markdown file {}", path.display()))?;
    let (html, warnings) = markdown_to_html_with_rewrites(&markdown, path, input_root, index_dirs);
    Ok(RenderedPage {
        html: rewrite_mermaid_blocks(&html),
        warnings,
    })
}

fn rewrite_mermaid_blocks(html: &str) -> String {
    let open_tag = "<pre><code class=\"language-mermaid\">";
    let close_tag = "</code></pre>";
    let mut output = String::with_capacity(html.len());
    let mut rest = html;

    while let Some(start) = rest.find(open_tag) {
        let (before, after_open) = rest.split_at(start);
        output.push_str(before);
        output.push_str("<pre class=\"mermaid\">");
        let after_open = &after_open[open_tag.len()..];
        if let Some(end) = after_open.find(close_tag) {
            let (code, after_close) = after_open.split_at(end);
            output.push_str(code);
            output.push_str("</pre>");
            rest = &after_close[close_tag.len()..];
        } else {
            output.push_str(after_open);
            rest = "";
            break;
        }
    }

    output.push_str(rest);
    output
}

fn markdown_to_html_with_rewrites(
    markdown: &str,
    source_path: &Path,
    input_root: &Path,
    index_dirs: &std::collections::HashSet<PathBuf>,
) -> (String, Vec<String>) {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_GFM);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_HEADING_ATTRIBUTES);
    options.insert(Options::ENABLE_SMART_PUNCTUATION);
    options.insert(Options::ENABLE_MATH);

    let mut warnings = Vec::new();
    let parser = Parser::new_ext(markdown, options).map(|event| match event {
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Link {
            link_type,
            dest_url: rewrite_link_dest(
                dest_url,
                source_path,
                input_root,
                index_dirs,
                &mut warnings,
            ),
            title,
            id,
        }),
        _ => event,
    });

    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    (html_output, warnings)
}

fn rewrite_link_dest<'a>(
    dest_url: CowStr<'a>,
    source_path: &Path,
    input_root: &Path,
    index_dirs: &std::collections::HashSet<PathBuf>,
    warnings: &mut Vec<String>,
) -> CowStr<'a> {
    let dest = dest_url.to_string();
    let Some((base, suffix)) = split_link(&dest) else {
        return dest_url;
    };
    if base.is_empty()
        || base.starts_with('#')
        || has_scheme(&base)
        || base.starts_with("mailto:")
        || base.starts_with("tel:")
    {
        return CowStr::from(dest);
    }

    let (resolved, relative_dir) = resolve_link_path(&base, source_path, input_root);
    if is_markdown_path(&base) {
        if !resolved.exists() {
            warnings.push(format!(
                "Missing link target: {} referenced from {}",
                base,
                source_path.display()
            ));
        }
        let replacement = replace_markdown_extension(&base);
        let mut replacement = if is_readme_path(&base) {
            let parent = relative_dir.unwrap_or_else(PathBuf::new);
            if index_dirs.contains(&parent) {
                replacement
            } else {
                readme_to_index(&base)
            }
        } else if is_index_path(&base) {
            replace_markdown_extension(&base)
        } else {
            replacement
        };
        replacement.push_str(&suffix);
        return CowStr::from(replacement);
    }

    CowStr::from(dest)
}

fn split_link(dest: &str) -> Option<(String, String)> {
    if dest.is_empty() {
        return None;
    }
    let mut chars = dest.char_indices();
    for (idx, ch) in &mut chars {
        if ch == '#' || ch == '?' {
            return Some((dest[..idx].to_string(), dest[idx..].to_string()));
        }
    }
    Some((dest.to_string(), String::new()))
}

fn resolve_link_path(base: &str, source_path: &Path, input_root: &Path) -> (PathBuf, Option<PathBuf>) {
    if base.starts_with('/') {
        let rel = PathBuf::from(base.trim_start_matches('/'));
        return (input_root.join(&rel), Some(rel.parent().unwrap_or(Path::new("")).to_path_buf()));
    }
    let source_dir = source_path.parent().unwrap_or(input_root);
    let resolved = source_dir.join(base);
    let relative_dir = resolved
        .parent()
        .and_then(|parent| parent.strip_prefix(input_root).ok())
        .map(|rel| rel.to_path_buf())
        .or_else(|| {
            if resolved.parent() == Some(input_root) {
                Some(PathBuf::new())
            } else {
                None
            }
        });
    (resolved, relative_dir)
}

fn has_scheme(dest: &str) -> bool {
    dest.starts_with("http://") || dest.starts_with("https://")
}

fn is_markdown_path(dest: &str) -> bool {
    matches!(
        Path::new(dest)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("md") | Some("markdown")
    )
}

fn is_readme_path(dest: &str) -> bool {
    let path = Path::new(dest);
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|s| s.to_ascii_lowercase());
    matches!(stem.as_deref(), Some("readme")) && is_markdown_path(dest)
}

fn is_index_path(dest: &str) -> bool {
    let path = Path::new(dest);
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|s| s.to_ascii_lowercase());
    matches!(stem.as_deref(), Some("index")) && is_markdown_path(dest)
}

fn replace_markdown_extension(dest: &str) -> String {
    let path = Path::new(dest);
    let stem = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or(dest);
    let mut base = String::new();
    let absolute = path.is_absolute();
    if let Some(parent) = path.parent() {
        let parent_str = parent.to_string_lossy();
        if parent != Path::new("") && parent_str != "." {
            if absolute {
                base.push('/');
                let trimmed = parent_str.trim_start_matches('/');
                if !trimmed.is_empty() {
                    base.push_str(trimmed);
                    base.push('/');
                }
            } else {
                if !parent_str.is_empty() {
                    base.push_str(parent_str.as_ref());
                    base.push('/');
                }
            }
        }
    }
    base.push_str(stem);
    base.push_str(".html");
    base
}

fn readme_to_index(dest: &str) -> String {
    let path = Path::new(dest);
    let mut base = String::new();
    let absolute = path.is_absolute();
    if let Some(parent) = path.parent() {
        let parent_str = parent.to_string_lossy();
        if parent != Path::new("") && parent_str != "." {
            if absolute {
                base.push('/');
                let trimmed = parent_str.trim_start_matches('/');
                if !trimmed.is_empty() {
                    base.push_str(trimmed);
                    base.push('/');
                }
            } else {
                if !parent_str.is_empty() {
                    base.push_str(parent_str.as_ref());
                    base.push('/');
                }
            }
        }
    }
    base.push_str("index.html");
    base
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_mermaid_code_fences() {
        let markdown = r#"
```mermaid
graph TD;
  A-->B;
```
"#;
        let index_dirs = std::collections::HashSet::new();
        let (html, _warnings) =
            markdown_to_html_with_rewrites(markdown, Path::new("."), Path::new("."), &index_dirs);
        let rewritten = rewrite_mermaid_blocks(&html);
        assert!(rewritten.contains(r#"<pre class="mermaid">"#));
        assert!(rewritten.contains("graph TD;"));
    }

    #[test]
    fn rewrites_markdown_links_to_html() {
        let root = tempfile::tempdir().expect("tempdir");
        let input_root = root.path();
        let docs_dir = input_root.join("docs");
        let guide_dir = docs_dir.join("guide");
        std::fs::create_dir_all(&guide_dir).expect("create dirs");
        std::fs::write(docs_dir.join("README.md"), "# Readme").expect("readme");
        std::fs::write(guide_dir.join("intro.md"), "# Intro").expect("intro");

        let markdown = r#"[Doc](guide/intro.md) and [Root](README.md)"#;
        let source = docs_dir.join("index.md");
        let mut index_dirs = std::collections::HashSet::new();
        index_dirs.insert(PathBuf::from("docs"));
        let (html, warnings) =
            markdown_to_html_with_rewrites(markdown, &source, input_root, &index_dirs);
        assert!(warnings.is_empty());
        assert!(html.contains("guide/intro.html"));
        assert!(html.contains("README.html"));
    }

    #[test]
    fn warns_on_missing_markdown_link() {
        let root = tempfile::tempdir().expect("tempdir");
        let input_root = root.path();
        let docs_dir = input_root.join("docs");
        std::fs::create_dir_all(&docs_dir).expect("create dirs");

        let markdown = r#"[Missing](missing.md)"#;
        let source = docs_dir.join("index.md");
        let index_dirs = std::collections::HashSet::new();
        let (_html, warnings) =
            markdown_to_html_with_rewrites(markdown, &source, input_root, &index_dirs);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("missing.md"));
    }
}
