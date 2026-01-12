use anyhow::{Context, Result};
use pulldown_cmark::{html, CowStr, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::path::{Path, PathBuf};

pub struct RenderedPage {
    pub html: String,
    pub warnings: Vec<String>,
    pub mode: DocMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocMode {
    Document,
    Slides,
}

#[derive(Default)]
struct FrontMatter {
    mode: Option<String>,
}

impl FrontMatter {
    fn is_slides(&self) -> bool {
        matches!(self.mode.as_deref(), Some("slides"))
    }
}

pub fn first_heading_title(markdown: &str) -> Option<String> {
    let (_front_matter, markdown) = parse_front_matter(markdown);
    let options = markdown_options(true);
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
    let (front_matter, content) = parse_front_matter(&markdown);
    if front_matter.is_slides() {
            let (html, warnings) =
                markdown_to_slides_with_rewrites(content, path, input_root, index_dirs);
        Ok(RenderedPage {
            html,
            warnings,
            mode: DocMode::Slides,
        })
    } else {
        let (html, warnings) = markdown_to_html_with_rewrites(content, path, input_root, index_dirs);
        Ok(RenderedPage {
            html: rewrite_mermaid_blocks(&html),
            warnings,
            mode: DocMode::Document,
        })
    }
}

fn parse_front_matter(markdown: &str) -> (FrontMatter, &str) {
    let mut front_matter = FrontMatter::default();
    let mut lines = markdown.split_inclusive('\n');
    let Some(first_line) = lines.next() else {
        return (front_matter, markdown);
    };
    let first_trimmed = first_line.trim_end_matches(&['\r', '\n'][..]);
    if first_trimmed != "---" {
        return (front_matter, markdown);
    }

    let mut offset = first_line.len();
    let mut front_lines: Vec<&str> = Vec::new();
    let mut end_offset: Option<usize> = None;

    for line in lines {
        let trimmed = line.trim_end_matches(&['\r', '\n'][..]);
        if trimmed == "---" {
            end_offset = Some(offset + line.len());
            break;
        }
        front_lines.push(trimmed);
        offset += line.len();
    }

    let Some(end_offset) = end_offset else {
        return (front_matter, markdown);
    };

    for line in front_lines {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            if key.trim() == "mode" {
                let value = value.trim().trim_matches(&['"', '\''][..]);
                if !value.is_empty() {
                    front_matter.mode = Some(value.to_ascii_lowercase());
                }
            }
        }
    }

    (front_matter, &markdown[end_offset..])
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
    let options = markdown_options(false);
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

fn markdown_to_slides_with_rewrites(
    markdown: &str,
    source_path: &Path,
    input_root: &Path,
    index_dirs: &std::collections::HashSet<PathBuf>,
) -> (String, Vec<String>) {
    let options = markdown_options(false);
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

    let mut slides: Vec<Vec<Event>> = Vec::new();
    let mut current: Vec<Event> = Vec::new();
    let mut pending: Vec<Event> = Vec::new();
    let mut seen_h1 = false;

    for event in parser {
        match event {
            Event::Start(Tag::Heading {
                level: HeadingLevel::H1,
                ..
            }) => {
                if !seen_h1 {
                    seen_h1 = true;
                    current.extend(pending.drain(..));
                } else if !current.is_empty() {
                    slides.push(current);
                    current = Vec::new();
                }
                current.push(event);
            }
            _ => {
                if seen_h1 {
                    current.push(event);
                } else {
                    pending.push(event);
                }
            }
        }
    }

    if seen_h1 {
        if !current.is_empty() {
            slides.push(current);
        }
    } else {
        slides.push(pending);
    }

    if slides.is_empty() {
        slides.push(Vec::new());
    }

    let slide_count = slides.len();
    let mut html_output = String::new();
    html_output.push_str(&format!(
        r#"<div class="slides-root" data-slide-count="{}" tabindex="0">"#,
        slide_count
    ));

    for (idx, events) in slides.into_iter().enumerate() {
        let mut slide_html = String::new();
        html::push_html(&mut slide_html, events.into_iter());
        let slide_html = rewrite_mermaid_blocks(&slide_html);
        let active_class = if idx == 0 { " is-active" } else { "" };
        let hidden_attr = if idx == 0 {
            ""
        } else {
            r#" aria-hidden="true""#
        };
        html_output.push_str(&format!(
            r#"<section class="slide{}" id="slide-{}" data-slide="{}"{}>"#,
            active_class,
            idx + 1,
            idx + 1,
            hidden_attr
        ));
        html_output.push_str(&slide_html);
        html_output.push_str("</section>");
    }

    html_output.push_str(&format!(
        r#"<div class="slides-progress">1 / {}</div>"#,
        slide_count
    ));
    html_output.push_str("</div>");

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

    let normalized_base = normalize_link_path(&base);
    let (resolved, relative_dir) = resolve_link_path(&normalized_base, source_path, input_root);
    if is_markdown_path(&normalized_base) {
        if !resolved.exists() {
            warnings.push(format!(
                "Missing link target: {} referenced from {}",
                normalized_base,
                source_path.display()
            ));
        }
        let replacement = replace_markdown_extension(&normalized_base);
        let mut replacement = if is_readme_path(&normalized_base) {
            let parent = relative_dir.unwrap_or_else(PathBuf::new);
            if index_dirs.contains(&parent) {
                replacement
            } else {
                readme_to_index(&normalized_base)
            }
        } else if is_index_path(&normalized_base) {
            replace_markdown_extension(&normalized_base)
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

fn normalize_link_path(path: &str) -> String {
    let is_absolute = path.starts_with('/');
    let mut parts: Vec<String> = Vec::new();
    for component in Path::new(path).components() {
        match component {
            std::path::Component::RootDir => {}
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if let Some(last) = parts.last() {
                    if last != ".." {
                        parts.pop();
                    } else if !is_absolute {
                        parts.push("..".to_string());
                    }
                } else if !is_absolute {
                    parts.push("..".to_string());
                }
            }
            std::path::Component::Normal(os) => {
                parts.push(os.to_string_lossy().to_string());
            }
            _ => {}
        }
    }

    if is_absolute {
        if parts.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", parts.join("/"))
        }
    } else if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

fn markdown_options(for_title_only: bool) -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_GFM);
    options.insert(Options::ENABLE_SMART_PUNCTUATION);
    if !for_title_only {
        options.insert(Options::ENABLE_TABLES);
        options.insert(Options::ENABLE_STRIKETHROUGH);
        options.insert(Options::ENABLE_TASKLISTS);
        options.insert(Options::ENABLE_FOOTNOTES);
        options.insert(Options::ENABLE_HEADING_ATTRIBUTES);
        options.insert(Options::ENABLE_MATH);
    }
    options
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

    #[test]
    fn uses_first_heading_as_title() {
        let markdown = "# First Title\n\n## Second Title\n";
        let title = first_heading_title(markdown);
        assert_eq!(title.as_deref(), Some("First Title"));
    }

    #[test]
    fn ignores_front_matter_in_title() {
        let markdown = "---\nmode: slides\n---\n# Deck Title\n";
        let title = first_heading_title(markdown);
        assert_eq!(title.as_deref(), Some("Deck Title"));
    }

    #[test]
    fn splits_slides_on_h1() {
        let markdown = "# One\n\nIntro\n\n# Two\n\nMore\n";
        let index_dirs = std::collections::HashSet::new();
        let (html, _warnings) =
            markdown_to_slides_with_rewrites(markdown, Path::new("."), Path::new("."), &index_dirs);
        assert!(html.contains(r#"data-slide-count="2""#));
        assert!(html.contains(r#"id="slide-1""#));
        assert!(html.contains(r#"id="slide-2""#));
        assert!(html.contains("One"));
        assert!(html.contains("Two"));
    }

    #[test]
    fn rewrites_absolute_markdown_links() {
        let root = tempfile::tempdir().expect("tempdir");
        let input_root = root.path();
        let guide_dir = input_root.join("guide");
        std::fs::create_dir_all(&guide_dir).expect("guide dir");
        std::fs::write(guide_dir.join("intro.md"), "# Intro").expect("intro");

        let markdown = r#"[Guide](/guide/intro.md)"#;
        let source = input_root.join("docs/index.md");
        let index_dirs = std::collections::HashSet::new();
        let (html, warnings) =
            markdown_to_html_with_rewrites(markdown, &source, input_root, &index_dirs);
        assert!(warnings.is_empty());
        assert!(html.contains(r#"/guide/intro.html"#));
    }

    #[test]
    fn rewrites_relative_markdown_links_with_fragments() {
        let root = tempfile::tempdir().expect("tempdir");
        let input_root = root.path();
        let docs_dir = input_root.join("docs");
        let guide_dir = input_root.join("guide");
        std::fs::create_dir_all(&docs_dir).expect("docs dir");
        std::fs::create_dir_all(&guide_dir).expect("guide dir");
        std::fs::write(guide_dir.join("intro.md"), "# Intro").expect("intro");

        let markdown = r#"[Guide](../guide/intro.md#part)"#;
        let source = docs_dir.join("index.md");
        let index_dirs = std::collections::HashSet::new();
        let (html, warnings) =
            markdown_to_html_with_rewrites(markdown, &source, input_root, &index_dirs);
        assert!(warnings.is_empty());
        assert!(html.contains(r#"../guide/intro.html#part"#));
    }

    #[test]
    fn rewrites_readme_to_index_when_no_index() {
        let root = tempfile::tempdir().expect("tempdir");
        let input_root = root.path();
        let docs_dir = input_root.join("docs");
        std::fs::create_dir_all(&docs_dir).expect("docs dir");
        std::fs::write(docs_dir.join("README.md"), "# Docs").expect("readme");

        let markdown = r#"[Docs](README.md)"#;
        let source = docs_dir.join("intro.md");
        let index_dirs = std::collections::HashSet::new();
        let (html, warnings) =
            markdown_to_html_with_rewrites(markdown, &source, input_root, &index_dirs);
        assert!(warnings.is_empty());
        assert!(html.contains(r#"index.html"#));
    }

    #[test]
    fn keeps_readme_html_when_index_exists() {
        let root = tempfile::tempdir().expect("tempdir");
        let input_root = root.path();
        let docs_dir = input_root.join("docs");
        std::fs::create_dir_all(&docs_dir).expect("docs dir");
        std::fs::write(docs_dir.join("README.md"), "# Docs").expect("readme");
        std::fs::write(docs_dir.join("index.md"), "# Index").expect("index");

        let markdown = r#"[Docs](README.md)"#;
        let source = docs_dir.join("intro.md");
        let mut index_dirs = std::collections::HashSet::new();
        index_dirs.insert(PathBuf::from("docs"));
        let (html, warnings) =
            markdown_to_html_with_rewrites(markdown, &source, input_root, &index_dirs);
        assert!(warnings.is_empty());
        assert!(html.contains(r#"README.html"#));
    }

    #[test]
    fn rewrites_markdown_extension_for_markdown_files() {
        let root = tempfile::tempdir().expect("tempdir");
        let input_root = root.path();
        let docs_dir = input_root.join("docs");
        std::fs::create_dir_all(&docs_dir).expect("docs dir");
        std::fs::write(docs_dir.join("note.markdown"), "# Note").expect("note");

        let markdown = r#"[Note](note.markdown)"#;
        let source = docs_dir.join("index.md");
        let index_dirs = std::collections::HashSet::new();
        let (html, warnings) =
            markdown_to_html_with_rewrites(markdown, &source, input_root, &index_dirs);
        assert!(warnings.is_empty());
        assert!(html.contains("note.html"));
    }

    #[test]
    fn normalizes_dot_segments_in_links() {
        let root = tempfile::tempdir().expect("tempdir");
        let input_root = root.path();
        let docs_dir = input_root.join("docs");
        let guide_dir = input_root.join("guide");
        std::fs::create_dir_all(&docs_dir).expect("docs dir");
        std::fs::create_dir_all(&guide_dir).expect("guide dir");
        std::fs::write(guide_dir.join("intro.md"), "# Intro").expect("intro");

        let markdown = r#"[Guide](../guide/./extra/../intro.md)"#;
        let source = docs_dir.join("index.md");
        let index_dirs = std::collections::HashSet::new();
        let (html, warnings) =
            markdown_to_html_with_rewrites(markdown, &source, input_root, &index_dirs);
        assert!(warnings.is_empty());
        assert!(html.contains("../guide/intro.html"));
    }

    #[test]
    fn ignores_fragment_only_links() {
        let markdown = r#"[Section](#part)"#;
        let (html, warnings) =
            markdown_to_html_with_rewrites(markdown, Path::new("."), Path::new("."), &std::collections::HashSet::new());
        assert!(warnings.is_empty());
        assert!(html.contains(r#"#part"#));
    }

    #[test]
    fn rewrites_dot_slash_readme() {
        let root = tempfile::tempdir().expect("tempdir");
        let input_root = root.path();
        let docs_dir = input_root.join("docs");
        std::fs::create_dir_all(&docs_dir).expect("docs dir");
        std::fs::write(docs_dir.join("README.md"), "# Docs").expect("readme");

        let markdown = r#"[Docs](./README.md)"#;
        let source = docs_dir.join("intro.md");
        let index_dirs = std::collections::HashSet::new();
        let (html, warnings) =
            markdown_to_html_with_rewrites(markdown, &source, input_root, &index_dirs);
        assert!(warnings.is_empty());
        assert!(html.contains("index.html"));
    }
}
