use anyhow::{Context, Result};
use std::path::Path;

pub struct Template {
    raw: String,
    style: String,
}

impl Template {
    pub fn built_in() -> Self {
        Self {
            raw: include_str!("../assets/theme/template.html").to_string(),
            style: include_str!("../assets/theme/style.css").to_string(),
        }
    }

    pub fn from_path(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read template {}", path.display()))?;
        warn_missing_placeholders(&raw, path);
        Ok(Self {
            raw,
            style: String::new(),
        })
    }

    pub fn render(
        &self,
        title: &str,
        content: &str,
        nav: &str,
        breadcrumbs: &str,
        extra_head: Option<&str>,
        extra_body: Option<&str>,
    ) -> String {
        render_template(
            &self.raw,
            &[
                ("{{title}}", title),
                ("{{content}}", content),
                ("{{nav}}", nav),
                ("{{breadcrumbs}}", breadcrumbs),
                ("{{style}}", &self.style),
                ("{{extra_head}}", extra_head.unwrap_or("")),
                ("{{extra_body}}", extra_body.unwrap_or("")),
            ],
        )
    }
}

fn render_template(template: &str, replacements: &[(&str, &str)]) -> String {
    let mut html = String::with_capacity(template.len());
    let mut rest = template;

    while let Some((index, placeholder, replacement)) = next_placeholder(rest, replacements) {
        html.push_str(&rest[..index]);
        html.push_str(replacement);
        rest = &rest[index + placeholder.len()..];
    }

    html.push_str(rest);
    html
}

fn next_placeholder<'a>(
    input: &str,
    replacements: &'a [(&'a str, &'a str)],
) -> Option<(usize, &'a str, &'a str)> {
    replacements
        .iter()
        .filter_map(|(placeholder, replacement)| {
            input
                .find(placeholder)
                .map(|index| (index, *placeholder, *replacement))
        })
        .min_by_key(|(index, _, _)| *index)
}

fn warn_missing_placeholders(template: &str, path: &Path) {
    let missing = missing_placeholders(template);
    if !missing.is_empty() {
        eprintln!(
            "Warning: template {} is missing placeholders: {}",
            path.display(),
            missing.join(", ")
        );
    }
}

fn missing_placeholders(template: &str) -> Vec<&'static str> {
    let required = [
        "{{title}}",
        "{{content}}",
        "{{nav}}",
        "{{breadcrumbs}}",
    ];
    let mut missing = Vec::new();
    for placeholder in &required {
        if !template.contains(placeholder) {
            missing.push(*placeholder);
        }
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::{missing_placeholders, Template};

    #[test]
    fn detects_missing_placeholders() {
        let template = "<html>{{title}}</html>";
        let missing = missing_placeholders(template);
        assert!(missing.contains(&"{{content}}"));
        assert!(missing.contains(&"{{nav}}"));
        assert!(missing.contains(&"{{breadcrumbs}}"));
    }

    #[test]
    fn does_not_replace_placeholders_inside_rendered_values() {
        let template = Template {
            raw: "<html>{{content}}<style>{{style}}</style></html>".to_string(),
            style: "body {}".to_string(),
        };

        let html = template.render(
            "Title",
            "<p><code>{{style}}</code> <code>{{nav}}</code></p>",
            "<nav>Nav</nav>",
            "<span>Home</span>",
            None,
            None,
        );

        assert!(html.contains("<style>body {}</style>"));
        assert!(html.contains("<code>{{style}}</code>"));
        assert!(html.contains("<code>{{nav}}</code>"));
        assert!(!html.contains("<code><nav>Nav</nav></code>"));
    }
}
