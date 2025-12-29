use anyhow::{Context, Result};
use pulldown_cmark::{html, Options, Parser};

pub fn markdown_to_html(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_GFM);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_HEADING_ATTRIBUTES);
    options.insert(Options::ENABLE_SMART_PUNCTUATION);
    options.insert(Options::ENABLE_MATH);

    let parser = Parser::new_ext(markdown, options);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    html_output
}

pub fn render_markdown_file(path: &std::path::Path) -> Result<String> {
    let markdown = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read markdown file {}", path.display()))?;
    let html = markdown_to_html(&markdown);
    Ok(rewrite_mermaid_blocks(&html))
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
        let html = markdown_to_html(markdown);
        let rewritten = rewrite_mermaid_blocks(&html);
        assert!(rewritten.contains(r#"<pre class="mermaid">"#));
        assert!(rewritten.contains("graph TD;"));
    }
}
