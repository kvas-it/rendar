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
        let mut html = self.raw.clone();
        html = html.replace("{{title}}", title);
        html = html.replace("{{content}}", content);
        html = html.replace("{{nav}}", nav);
        html = html.replace("{{breadcrumbs}}", breadcrumbs);
        html = html.replace("{{style}}", &self.style);
        html = html.replace("{{extra_head}}", extra_head.unwrap_or(""));
        html = html.replace("{{extra_body}}", extra_body.unwrap_or(""));
        html
    }
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
    use super::missing_placeholders;

    #[test]
    fn detects_missing_placeholders() {
        let template = "<html>{{title}}</html>";
        let missing = missing_placeholders(template);
        assert!(missing.contains(&"{{content}}"));
        assert!(missing.contains(&"{{nav}}"));
        assert!(missing.contains(&"{{breadcrumbs}}"));
    }
}
